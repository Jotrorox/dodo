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

#[test]
fn static_atomics_share_state_without_unsafe() {
    let scratch = std::env::temp_dir().join(format!("dodo-sync-static-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let dodo = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(args)
            .output()
            .unwrap()
    };
    // A constructor call initializes a static atomic, which the program then
    // changes through shared references, from several threads too.
    let source = scratch.join("shared.dodo");
    fs::write(
        &source,
        "package main\nimport \"std/sync/atomic\"\nimport \"std/thread\"\n\
         static HITS: atomic.Atomic<u32> = atomic.Atomic.new(5u32)\n\
         static FLAGS: [2]atomic.Atomic<u8> = [atomic.Atomic.new(1u8), atomic.Atomic.new(2u8)]\n\
         struct Counter {\n pub count: u64\n pub fn new(count: u64) -> Counter {\n return Counter { count }\n }\n}\n\
         static START: Counter = Counter.new(7u64)\n\
         pub struct Bump {\n pub fn run(self) -> u32 {\n for _ in 0..1000 {\n HITS.fetch_add(1, atomic.Ordering.Relaxed)!\n }\n return 0\n }\n}\n\
         fn main() {\n first := thread.Storage.new::<Bump, u32>()\n second := thread.Storage.new::<Bump, u32>()\n\
         a := thread.spawn(&mut first, Bump {})!\n b := thread.spawn(&mut second, Bump {})!\n\
         Bump {}.run()\n a.join()\n b.join()\n core.assert_eq(HITS.load(atomic.Ordering.SeqCst)!, 3005u32)\n\
         FLAGS[1].store(9, atomic.Ordering.Release)!\n core.assert_eq(FLAGS[1].load(atomic.Ordering.Acquire)!, 9u8)\n\
         core.assert_eq(FLAGS[0].load(atomic.Ordering.Acquire)!, 1u8)\n core.assert_eq(START.count, 7u64)\n}\n",
    )
    .unwrap();
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!(
            "shared-{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let output = dodo(&[
            "build",
            source.to_str().unwrap(),
            "-O",
            optimization,
            "-o",
            executable.to_str().unwrap(),
        ]);
        success(output, "compile static atomics");
        execute_bounded(&executable);
    }
    // In firmware the static lives in RAM, not flash, and an interrupt
    // handler updates it with exclusive loads and stores.
    let firmware = scratch.join("firmware.dodo");
    fs::write(
        &firmware,
        "package firmware\nimport \"std/sync/atomic\"\n\
         static TICKS: atomic.Atomic<u32> = atomic.Atomic.new(0u32)\n\
         pub fn tick() {\n TICKS.fetch_add(1, atomic.Ordering.Relaxed)!\n}\n\
         pub fn ticks() -> u32 {\n return TICKS.load(atomic.Ordering.Acquire)!\n}\n",
    )
    .unwrap();
    let assembly = scratch.join("firmware.s");
    success(
        dodo(&[
            "build",
            firmware.to_str().unwrap(),
            "--target",
            "thumbv8m.main-none-eabi",
            "--emit",
            "asm",
            "-O",
            "2",
            "-o",
            assembly.to_str().unwrap(),
        ]),
        "compile firmware atomics",
    );
    let text = fs::read_to_string(&assembly).unwrap();
    assert!(text.contains("ldrex") && text.contains("strex"), "{text}");
    assert!(!text.contains(".rodata"), "{text}");
    // Constants are copied at each use, so they cannot hold atomics; only
    // constructors that return a struct literal are constant.
    for (name, body, expected) in [
        (
            "const_atomic",
            "const HITS: atomic.Atomic<u32> = atomic.Atomic.new(0u32)\nfn main() {}",
            "a `const` cannot hold an atomic",
        ),
        (
            "computed_constructor",
            "struct Pair {\n pub a: u32\n pub fn new(a: u32) -> Pair {\n doubled := a * 2\n return Pair { a: doubled }\n }\n}\nstatic P: Pair = Pair.new(1u32)\nfn main() {}",
            "compile-time constants",
        ),
        (
            "unsafe_constructor",
            "struct Pair {\n pub a: u32\n pub unsafe fn new(a: u32) -> Pair {\n return Pair { a }\n }\n}\nstatic P: Pair = unsafe { Pair.new(1u32) }\nfn main() {}",
            "compile-time constants|unsafe",
        ),
        (
            "runtime_argument",
            "fn seed() -> u32 {\n return 3\n}\nstatic HITS: atomic.Atomic<u32> = atomic.Atomic.new(seed())\nfn main() {}",
            "compile-time constants",
        ),
    ] {
        let source = scratch.join(format!("{name}.dodo"));
        fs::write(
            &source,
            format!("package rejected\nimport \"std/sync/atomic\"\n{body}\n"),
        )
        .unwrap();
        let output = dodo(&["check", source.to_str().unwrap()]);
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted {name}");
        assert!(
            expected.split('|').any(|part| diagnostic.contains(part)),
            "{name}: expected {expected}, got {diagnostic}"
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
