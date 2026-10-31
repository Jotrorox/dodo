//! Process signals delivered through a pollable wake channel.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-signal-{}-{}",
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
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/signal/{name}.dodo"))
}
fn compile(source: &Path, output: &Path, optimization: &str) {
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .args(["-O", optimization, "-o"])
            .arg(output)
            .output()
            .unwrap(),
        &format!("compile {} at O{optimization}", source.display()),
    );
}

#[test]
fn listener_lifecycle_at_o0_and_o3() {
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let binary = workspace.0.join(format!(
            "lifecycle-{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        compile(&fixture("lifecycle"), &binary, optimization);
        success(
            Command::new(binary).output().unwrap(),
            &format!("run lifecycle at O{optimization}"),
        );
    }
}

#[test]
fn windows_signal_adapter_cross_compiles_at_o0_and_o3() {
    let workspace = Workspace::new();
    for name in ["lifecycle", "event_loop", "reactor_shutdown"] {
        for optimization in ["0", "3"] {
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(fixture(name))
                    .args([
                        "--target",
                        "x86_64-pc-windows-msvc",
                        "--emit",
                        "obj",
                        "-O",
                        optimization,
                        "-o",
                    ])
                    .arg(workspace.0.join(format!("{name}-{optimization}.obj")))
                    .output()
                    .unwrap(),
                "Win64 signal adapter object",
            );
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn raised_signals_keep_order_overflow_and_restore_dispositions() {
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let binary = workspace.0.join(format!("raise-{optimization}"));
        compile(&fixture("raise_checks"), &binary, optimization);
        success(
            Command::new(binary).output().unwrap(),
            &format!("run raise_checks at O{optimization}"),
        );
    }
}

/// Runs a fixture that prints `ready` once it listens, then sends each signal
/// only after the previous one was reported, as an operator or service
/// manager would, so none of them can coalesce.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn drive(name: &str, steps: &[(&str, &str)]) {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;

    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let binary = workspace.0.join(format!("{name}-{optimization}"));
        compile(&fixture(name), &binary, optimization);
        let mut child = Command::new(&binary)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let pid = child.id().to_string();
        let (lines, received) = mpsc::channel();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let reader = std::thread::spawn(move || {
            for line in stdout.lines() {
                if lines.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut expect = |wanted: &str| match received.recv_timeout(Duration::from_secs(10)) {
            Ok(line) => assert_eq!(line, wanted, "{name} at O{optimization}"),
            Err(error) => {
                let _ = child.kill();
                panic!("{name} at O{optimization}: no `{wanted}` line: {error}");
            }
        };
        expect("ready");
        for (signal, line) in steps {
            success(
                Command::new("kill")
                    .args([*signal, pid.as_str()])
                    .output()
                    .unwrap(),
                "send signal",
            );
            expect(line);
        }
        let status = child.wait().unwrap();
        assert!(status.success(), "{name} at O{optimization}: {status}");
        reader.join().unwrap();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn external_signals_wake_a_poll_loop() {
    drive(
        "event_loop",
        &[
            ("-HUP", "hangup"),
            ("-HUP", "hangup"),
            ("-INT", "interrupt cancelled"),
            ("-TERM", "terminate cancelled"),
        ],
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn terminate_stops_a_reactor_gracefully() {
    drive("reactor_shutdown", &[("-TERM", "stopped")]);
}
