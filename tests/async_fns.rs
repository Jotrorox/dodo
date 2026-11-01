//! `async fn` and `await`, std/embedded/sync, and their firmware builds:
//! host execution of the fixtures at both optimization extremes, the
//! diagnostics for misuse, the trap for a moved suspended future, and the
//! examples for every board.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn dodo(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(args)
        .output()
        .unwrap()
}

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-async-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_fixture(fixture: &str) {
    let scratch = Scratch::new(fixture);
    let source = root().join("tests/stdlib").join(format!("{fixture}.dodo"));
    for optimization in ["0", "3"] {
        let executable = scratch.path(&format!(
            "{fixture}-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            &dodo(&[
                "build",
                source.to_str().unwrap(),
                "-O",
                optimization,
                "-o",
                &executable,
            ]),
            &format!("compile {fixture} at -O{optimization}"),
        );
        success(
            &Command::new(&executable).output().unwrap(),
            &format!("execute {fixture} at -O{optimization}"),
        );
    }
}

#[test]
fn async_fns_run_on_the_executor_against_the_simulated_clock() {
    run_fixture("async_checks");
}

#[test]
fn mutexes_share_values_and_take_the_cross_core_lock() {
    run_fixture("embedded_sync_checks");
}

#[test]
fn async_fns_build_with_debug_information() {
    let scratch = Scratch::new("debug");
    let source = root().join("tests/stdlib/async_checks.dodo");
    let executable = scratch.path(&format!("debug{}", std::env::consts::EXE_SUFFIX));
    success(
        &dodo(&["build", source.to_str().unwrap(), "-g", "-o", &executable]),
        "compile with -g",
    );
    success(
        &Command::new(&executable).output().unwrap(),
        "execute the -g build",
    );
}

/// Check `source` with `command` (`check` or `build`) and return stderr,
/// asserting that it fails.
fn rejected(command: &str, source: &str) -> String {
    let scratch = Scratch::new("rejected");
    let path = scratch.write("main.dodo", source);
    let output_path = scratch.path("main.o");
    let mut args = vec![command, path.to_str().unwrap()];
    if command == "build" {
        args.extend(["--emit", "obj", "-o", &output_path]);
    }
    let output = dodo(&args);
    assert!(
        !output.status.success(),
        "expected a rejection:\n{source}\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stderr).unwrap()
}

const PRELUDE: &str = "package main\nimport \"std/embedded/executor\"\n";

#[test]
fn misuse_of_async_and_await_is_diagnosed() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "check",
            "fn plain() { await executor.sleep_ms(1) }\nfn main() { plain() }",
            "`await` is only allowed in the body of an `async fn`",
        ),
        (
            "check",
            "async fn value() -> u32 { return 1 }\nfn take(x: u32) {}\nasync fn caller() { take(await value()) }\nfn main() {}",
            "`await` must begin its statement",
        ),
        (
            "check",
            "async fn caller() {\n    run := fn() { await executor.sleep_ms(1) }\n    run()\n}\nfn main() {}",
            "`await` must begin its statement",
        ),
        (
            "check",
            "interface Waiter {\n    async fn wait(&mut self)\n}\nfn main() {}",
            "interface methods cannot be `async`",
        ),
        (
            "check",
            "async fn caller() { await 5u32 }\nfn main() {}",
            "cannot await a value of type `u32`",
        ),
        (
            "check",
            "struct Nothing { x: u32 }\nasync fn caller() { await Nothing { x: 1 } }\nfn main() {}",
            "it has no `poll` method",
        ),
        (
            "check",
            "struct TickFuture { x: u32 }\nasync fn tick() {}\nfn main() {}",
            "names its future type `TickFuture`, which is already declared",
        ),
        (
            "check",
            "@test\nasync fn waits() {}\nfn main() {}",
            "a test cannot be async",
        ),
        (
            "check",
            "unsafe async fn risky() {}\nfn main() {}",
            "an async fn cannot be unsafe",
        ),
        (
            "check",
            "async struct Wrong { x: u32 }\nfn main() {}",
            "`async` applies only to functions",
        ),
        (
            "check",
            "enum Fault { Bad }\nasync fn risky() -> u32!Fault { return ok(1) }\nasync fn caller() { await risky() }\nfn main() {}",
            "Result must be handled",
        ),
        (
            "build",
            "async fn forever(n: u32) -> u32 {\n    if n == 0 {\n        return 0\n    }\n    return await forever(n - 1)\n}\nfn main() {}",
            "holds itself, so it would be infinitely large",
        ),
    ];
    for (command, body, expected) in cases {
        let source = format!("{PRELUDE}{body}\n");
        let stderr = rejected(command, &source);
        assert!(
            stderr.contains(expected),
            "expected `{expected}` for:\n{source}\ngot:\n{stderr}"
        );
    }
}

#[test]
fn async_borrows_follow_the_future() {
    // The future keeps the borrow of `count` until it is dropped.
    let stderr = rejected(
        "check",
        &format!(
            "{PRELUDE}async fn bump(count: &mut u32) {{ *count += 1 }}\nfn main() {{\n    count := 0u32\n    future := bump(&mut count)\n    count += 1\n    core.drop(future)\n}}\n"
        ),
    );
    assert!(stderr.contains("count"), "{stderr}");
}

#[test]
fn moving_a_suspended_future_traps() {
    let scratch = Scratch::new("moved");
    let source = scratch.write(
        "main.dodo",
        r#"package main
import "std/embedded/executor"
import "std/embedded/hal/fake"

async fn wait_twice() {
    buffer := [1u8; 4]
    view := &buffer
    await executor.sleep_ms(1)
    await executor.sleep_ms(1)
    core.assert(view[0] == 1)
}

fn poll_once<F: executor.Task>(future: &mut F, clock: &mut fake.Clock) {
    runner := executor.Executor.new()
    tasks: [1]&mut dyn executor.Task = [future]
    runner.run_until(&mut tasks, clock, 500)
}

fn main() {
    clock := fake.Clock.new()
    first := wait_twice()
    poll_once(&mut first, &mut clock)
    moved := first
    poll_once(&mut moved, &mut clock)
}
"#,
    );
    let executable = scratch.path(&format!("moved{}", std::env::consts::EXE_SUFFIX));
    success(
        &dodo(&["build", source.to_str().unwrap(), "-o", &executable]),
        "compile the moving program",
    );
    let output = Command::new(&executable).output().unwrap();
    assert!(
        !output.status.success(),
        "moving a suspended future must trap"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("async future moved"), "{stderr}");
}

// Async tasks, mutexes shared with interrupt handlers, and the doorbells on
// every board. Needs no linker.
#[test]
fn async_and_sync_firmware_compiles_for_every_board() {
    let scratch = Scratch::new("firmware");
    let doorbell = scratch.write(
        "doorbell.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/executor"
import "std/embedded/sync"

const WAKE: executor.Signal = executor.Signal { id: 3 }
static COUNT: sync.Mutex<u32> = sync.Mutex.new(0u32)

async fn wait_for_rings(times: u32) {
    for _ in 0u32..times {
        await WAKE.wait()
        COUNT.with(fn(count: &mut u32) { *count += 1 })
    }
}

fn main() {
    b := board.take()!
    clock := b.timer.alarm(0)!
    match b.doorbells.claim(2) {
        ok(claimed) => {
            bell := claimed
            bell.set_signal(&WAKE)
            bell.ring()
            if bell.answer() {
                bell.withdraw()
            }
        }
        err(_) => {}
    }
    waiter := wait_for_rings(1)
    tasks: [1]&mut dyn executor.Task = [&mut waiter]
    executor.run(&mut tasks, &mut clock)
}
"#,
    );
    let mut inputs = vec![
        (root().join("examples/async_tasks"), "async_tasks"),
        (root().join("examples/interrupts"), "interrupts"),
    ];
    inputs.push((doorbell.clone(), "doorbell"));
    for board in ["pico", "pico2", "pico2_w"] {
        for (input, name) in &inputs {
            // The RP2040's board has no doorbells part.
            if *name == "doorbell" && board == "pico" {
                continue;
            }
            for level in ["0", "2"] {
                let object = scratch.path(&format!("{name}-{board}-O{level}.o"));
                success(
                    &dodo(&[
                        "build",
                        input.to_str().unwrap(),
                        "--board",
                        board,
                        "-O",
                        level,
                        "--emit",
                        "obj",
                        "-o",
                        &object,
                    ]),
                    &format!("{name} for {board} at -O{level}"),
                );
                assert!(Path::new(&object).exists());
            }
        }
    }
}
