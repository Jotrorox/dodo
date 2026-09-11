//! Native thread ownership, checked transfer, atomics and failure cleanup.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-thread-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn run_bounded(executable: &Path) -> Output {
    let mut child = Command::new(executable)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let result = child.wait_with_output().unwrap();
            panic!(
                "thread fixture exceeded 30 second deadline: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn native_threads_join_detach_contention_and_destruction() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = Workspace::new();
    for level in ["0", "3"] {
        let executable = workspace
            .0
            .join(format!("threads-{level}{}", std::env::consts::EXE_SUFFIX));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/os/thread_checks.dodo"))
                .args(["-O", level, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile thread ownership fixture",
        );
        success(run_bounded(&executable), "run thread ownership fixture");
    }
}

#[test]
#[cfg(target_os = "linux")]
fn startup_and_allocation_failures_destroy_tasks_and_release_mappings() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = Workspace::new();
    for level in ["0", "3"] {
        let executable = workspace.0.join(format!("failure-{level}"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/os/thread_failures.dodo"))
                .args(["-O", level, "--link-arg"])
                .arg(root.join("tests/os/thread_failures.c"))
                .args([
                    "--link-arg",
                    "-Wl,--wrap=pthread_create",
                    "--link-arg",
                    "-Wl,--wrap=mmap",
                    "--link-arg",
                    "-Wl,--wrap=munmap",
                    "-o",
                ])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile injected thread failures",
        );
        success(run_bounded(&executable), "run injected thread failures");
    }
}

#[test]
fn cross_thread_transfer_and_scoped_storage_rejections() {
    let workspace = Workspace::new();
    for (name, body, expected) in [
        (
            "reference",
            "struct Bad { value: &usize }\nfn main() { mem.assert_send::<Bad>() }",
            "cross-thread transfer",
        ),
        (
            "mutable_reference",
            "struct Bad { value: &mut usize }\nfn main() { mem.assert_send::<Bad>() }",
            "cross-thread transfer",
        ),
        (
            "raw",
            "struct Bad { pointer: *mut usize }\nfn main() { mem.assert_send::<Bad>() }",
            "cross-thread transfer",
        ),
        (
            "raw_nested",
            "struct Inner { pointer: *const usize }\nstruct Bad { inner: [2]Inner }\nfn main() { mem.assert_send::<Bad>() }",
            "cross-thread transfer",
        ),
        (
            "opaque",
            "fn main() { mem.assert_send::<MaybeUninit<usize>>() }",
            "cross-thread transfer",
        ),
        (
            "destructor",
            "struct Bad { fn drop(&mut self) {} }\nfn main() { mem.assert_send::<Bad>() }",
            "cross-thread transfer",
        ),
        (
            "unsafe_borrow_override",
            "@unsafe_send\nstruct Bad { value: &usize }\nfn main() { mem.assert_send::<Bad>() }",
            "cross-thread transfer",
        ),
        (
            "sharing",
            "struct Bad { value: *mut usize }\nfn main() { mem.assert_sync::<Bad>() }",
            "cross-thread sharing",
        ),
        (
            "allocator",
            "import \"alloc/shared_arena\"\nfn main() { mem.assert_send::<shared_arena.Handle>() }",
            "cross-thread transfer",
        ),
        (
            "result_output",
            "pub struct Task { pub fn run(self) -> usize!u8 { return ok(1) } }\nfn main() { s := thread.Storage.new::<Task, usize!u8>()\nmatch thread.spawn(&mut s, Task {}) { ok(worker) => { match worker.join() { ok(_) => {}, err(_) => {} } }, err(_) => {} } }",
            "Results",
        ),
        (
            "storage_escape",
            "pub struct Task { pub fn run(self) -> usize { return 1 } }\nfn escape() -> thread.Join<Task, usize>!thread.ThreadError { s := thread.Storage.new::<Task, usize>()\nreturn thread.spawn(&mut s, Task {}) }\nfn main() {}",
            "borrow",
        ),
        (
            "storage_reuse",
            "pub struct Task { pub fn run(self) -> usize { return 1 } }\nfn main() { s := thread.Storage.new::<Task, usize>()\nmatch thread.spawn(&mut s, Task {}) { ok(worker) => { s = thread.Storage.new::<Task, usize>()\nworker.join() }, err(_) => {} } }",
            "borrow",
        ),
        (
            "callback_unsafe",
            "unsafe fn work(p: *mut u8) {}\nfn main() { mem.callback(work) }",
            "unsafe block",
        ),
        (
            "callback_signature",
            "unsafe fn work(p: usize) {}\nfn main() { unsafe { mem.callback(work) } }",
            "unsafe fn(*mut u8) -> void",
        ),
    ] {
        let source = workspace.0.join(format!("{name}.dodo"));
        fs::write(
            &source,
            format!("package rejected\nimport \"std/thread\"\nimport \"core/mem\"\n{body}\n"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted {name}");
        assert!(
            message.contains(expected),
            "{name}: expected {expected:?}, got {message}"
        );
        assert!(!message.contains("panicked"), "{name}: {message}");
    }
}

#[test]
fn atomic_ordering_and_target_capabilities_are_checked() {
    let workspace = Workspace::new();
    for (name, operation, expected) in [
        ("load_release", "mem.atomic_load(p, 2)", "memory ordering"),
        (
            "store_acquire",
            "mem.atomic_store(p, 1usize, 1)",
            "memory ordering",
        ),
        (
            "failure_release",
            "mem.atomic_compare_exchange(p, 0usize, 1usize, 4, 2)",
            "failure ordering",
        ),
        (
            "failure_stronger",
            "mem.atomic_compare_exchange(p, 0usize, 1usize, 0, 1)",
            "failure ordering",
        ),
        (
            "dynamic",
            "mem.atomic_load(p, ordering)",
            "compile-time integer",
        ),
        ("invalid", "mem.atomic_load(p, 5)", "compile-time integer"),
    ] {
        let source = workspace.0.join(format!("{name}.dodo"));
        fs::write(&source, format!("package invalid\nimport \"core/mem\"\nunsafe fn operation(p: *mut usize, ordering: usize) {{ unsafe {{ {operation} }} }}\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted {name}");
        assert!(message.contains(expected), "{name}: {message}");
    }
    let source = workspace.0.join("unsupported.dodo");
    fs::write(&source, "package unsupported\nimport \"core/mem\"\npub unsafe fn read(p: *const usize) -> usize { unsafe { return mem.atomic_load(p, 0) } }\n").unwrap();
    for target in ["thumbv6m-none-eabi", "wasm32-unknown-unknown"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(["--target", target, "--emit", "obj", "-o"])
            .arg(workspace.0.join("unsupported.o"))
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "accepted unsupported atomic target {target}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("not supported for target"));
    }
}
