//! Native synchronization with bounded waits and checked guard ownership.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn execute_bounded(path: &Path) {
    let mut child = Command::new(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            success(
                child.wait_with_output().unwrap(),
                "execute synchronization fixture",
            );
            return;
        }
        if Instant::now() >= end {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "synchronization fixture exceeded 30 seconds: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn native_synchronization_handshakes_contention_and_channels() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-sync-native-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!(
            "sync-{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/os/sync_checks.dodo"))
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile synchronization fixture",
        );
        execute_bounded(&executable);
    }
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn guards_payloads_and_result_obligations_are_checked() {
    let scratch = std::env::temp_dir().join(format!("dodo-sync-reject-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for (name, body, expected) in [
        (
            "guard_transfer",
            "fn main() { mem.assert_send::<allocated.Guard<usize>>() }",
            "cross-thread transfer",
        ),
        (
            "read_guard_transfer",
            "fn main() { mem.assert_send::<allocated.ReadGuard<usize>>() }",
            "cross-thread transfer",
        ),
        (
            "borrowed_payload",
            "fn main() { value := 1usize\nmatch allocated.Mutex.new(allocated.PageAllocator.new(), &value) { ok(_) => {}, err(_) => {} } }",
            "cross-thread transfer|checked-borrow",
        ),
        (
            "raw_payload",
            "fn main() { value := 1usize\nmatch allocated.Channel.new::<*const usize>(allocated.PageAllocator.new(), 1) { ok(_) => {}, err(_) => {} } }",
            "cross-thread transfer",
        ),
        (
            "result_payload",
            "fn main() { match allocated.Mutex.new::<usize!u8>(allocated.PageAllocator.new(), ok(1)) { ok(_) => {}, err(_) => {} } }",
            // Generic checking can reject either the opaque storage operation
            // or the allocation-failure exit that would discard the Result.
            "unhandled Results|Result `value` is left unhandled",
        ),
        (
            "escaping_guard",
            "fn escape(lock: &allocated.Mutex<usize>) -> &usize!error.Error from(lock) { guard := lock.lock(0)?\nreturn ok(guard.get()) }\nfn main() {}",
            "borrow",
        ),
        (
            "unlock_live_view",
            "fn test() -> void!error.Error { lock := allocated.Mutex.new(allocated.PageAllocator.new(), 1usize)?\nguard := lock.lock(0)?\nvalue := guard.get()\ncore.drop(guard)\nif *value == 1 {}\nreturn ok() }\nfn main() {}",
            "borrow",
        ),
        (
            "wait_live_view",
            "fn test() -> void!error.Error { lock := allocated.Mutex.new(allocated.PageAllocator.new(), 1usize)?\nguard := lock.lock(0)?\nvalue := guard.get()\nguard.wait(0)?\nif *value == 1 {}\nreturn ok() }\nfn main() {}",
            "borrow",
        ),
        (
            "ignored_lock_result",
            "fn main() { match allocated.Mutex.new(allocated.PageAllocator.new(), 1usize) { ok(lock) => { lock.lock(0) }, err(_) => {} } }",
            "Result",
        ),
        (
            "private_storage",
            "fn main() { lock := allocated.Mutex<usize> { address: 0, pointer: 0 } }",
            "private",
        ),
    ] {
        let source = scratch.join(format!("{name}.dodo"));
        fs::write(&source, format!("package rejected\nimport \"core/mem\"\nimport \"std/sync/allocated\"\nimport \"std/sync/error\"\n{body}\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
            .output()
            .unwrap();
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted {name}");
        assert!(
            expected.split('|').any(|part| diagnostic.contains(part)),
            "{name}: expected {expected}, got {diagnostic}"
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
