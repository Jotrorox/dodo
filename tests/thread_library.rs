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
#[cfg(any(target_os = "linux", target_os = "macos"))]
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
                // macOS replaces these by definition instead; see the fixture.
                .args(if cfg!(target_os = "macos") {
                    &[][..]
                } else {
                    &[
                        "--link-arg",
                        "-Wl,--wrap=pthread_create",
                        "--link-arg",
                        "-Wl,--wrap=mmap",
                        "--link-arg",
                        "-Wl,--wrap=munmap",
                    ][..]
                })
                .arg("-o")
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
            "@unsafe_send\nunsafe struct Bad { value: &usize }\nfn main() { mem.assert_send::<Bad>() }",
            "cross-thread transfer",
        ),
        (
            "contract_without_unsafe",
            "@unsafe_sync\nstruct Bad { pointer: *mut usize }\nfn main() { mem.assert_sync::<Bad>() }",
            "declare the struct as `unsafe struct`",
        ),
        (
            "unsafe_without_contract",
            "unsafe struct Bad { pointer: *mut usize }\nfn main() {}",
            "`unsafe` on a struct applies only with @unsafe_send or @unsafe_sync",
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
    let build = |name: &str, text: &str, target: &str, emit: &str| {
        let source = workspace.0.join(format!("{name}.dodo"));
        fs::write(&source, text).unwrap();
        let output = workspace.0.join(format!("{name}-{target}.{emit}"));
        let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(["--target", target, "--emit", emit, "-O", "2", "-o"])
            .arg(&output)
            .output()
            .unwrap();
        (result, output)
    };
    let load = "package atomics\nimport \"core/mem\"\npub unsafe fn read(p: *const u32) -> u32 { unsafe { return mem.atomic_load(p, 4) } }\npub unsafe fn write(p: *mut u8, v: u8) { unsafe { mem.atomic_store(p, v, 2) } }\n";
    let add = "package atomics\nimport \"core/mem\"\npub unsafe fn add(p: *mut u32) -> u32 { unsafe { return mem.atomic_fetch_add(p, 1u32, 0) } }\n";
    // No OS or libatomic fallback is inserted where the CPU has no atomics.
    let (output, _) = build("load", load, "wasm32-unknown-unknown", "obj");
    assert!(!output.status.success(), "accepted atomics on wasm32");
    assert!(String::from_utf8_lossy(&output.stderr).contains("not supported for target"));
    // ARMv7-M and ARMv8-M have exclusive loads and stores; the Cortex-M33
    // also has load-acquire and store-release.
    for (target, expected) in [
        (
            "thumbv8m.main-none-eabi",
            &["lda\t", "stlb", "ldrex", "strex"][..],
        ),
        (
            "thumbv7em-none-eabihf",
            &["ldr", "strb", "ldrex", "strex", "dmb"][..],
        ),
    ] {
        let mut text = String::new();
        for (name, source) in [("load", load), ("add", add)] {
            let (output, assembly) = build(name, source, target, "asm");
            assert!(
                output.status.success(),
                "{target}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            text.push_str(&fs::read_to_string(assembly).unwrap());
        }
        for instruction in expected {
            assert!(
                text.contains(instruction),
                "{target} lacks {instruction}:\n{text}"
            );
        }
        assert!(
            !text.contains("__atomic") && !text.contains("__sync"),
            "{text}"
        );
    }
    // ARMv6-M (the RP2040's Cortex-M0+) has atomic loads and stores only.
    let (output, assembly) = build("load", load, "thumbv6m-none-eabi", "asm");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(assembly).unwrap();
    assert!(text.contains("dmb") && !text.contains("__atomic"), "{text}");
    let (output, _) = build("add", add, "thumbv6m-none-eabi", "obj");
    assert!(
        !output.status.success(),
        "accepted read-modify-write on ARMv6-M"
    );
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("atomic `fetch_add` is not supported for target `thumbv6m-none-eabi`")
            && message.contains("critical section"),
        "{message}"
    );
    // Only reachable operations are diagnosed, so packages stay portable.
    let unused = "package atomics\nimport \"core/mem\"\nunsafe fn add(p: *mut u32) -> u32 { unsafe { return mem.atomic_fetch_add(p, 1u32, 0) } }\npub unsafe fn read(p: *const u32) -> u32 { unsafe { return mem.atomic_load(p, 0) } }\n";
    for target in ["thumbv6m-none-eabi", "wasm32-unknown-unknown"] {
        let (output, _) = build(
            "unused",
            &unused.replace("mem.atomic_load(p, 0)", "*p"),
            target,
            "obj",
        );
        assert!(
            output.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let typed = "package atomics\nimport \"std/sync/atomic\"\npub fn get(a: &atomic.Atomic<u32>) -> u32 { return a.load(atomic.Ordering.SeqCst)! }\npub fn put(a: &atomic.Atomic<u16>, v: u16) { a.store(v, atomic.Ordering.Release)! }\n";
    let (output, _) = build("typed", typed, "thumbv6m-none-eabi", "obj");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
