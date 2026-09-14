//! Hosted standard streams, redirection, native failures, and safe borrowing.
use dodoc::package;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-console-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn source(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(format!("{name}.dodo"));
        fs::write(&path, body).unwrap();
        path
    }
    fn build(&self, source: &Path, optimization: &str) -> PathBuf {
        let exe = self.0.join(format!(
            "{}-{optimization}{}",
            source.file_stem().unwrap().to_str().unwrap(),
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(source)
                .args(["-O", optimization, "-o"])
                .arg(&exe)
                .output()
                .unwrap(),
        );
        exe
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn run(exe: &Path, input: &[u8]) -> Output {
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    success(child.wait_with_output().unwrap())
}

#[test]
fn console_examples_and_borrowed_stream_ownership() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = Workspace::new();
    for optimization in ["0", "3"] {
        for (source, input, stdout, stderr) in [
            (
                "tests/os/console_checks.dodo",
                "abc\r\ntail",
                "Hello, world!\n42\ntext line\n42\n",
                "io: Closed code=9 transferred=0\n",
            ),
            ("examples/hello.dodo", "", "Hello, world!\n", ""),
            (
                "examples/console_formatting.dodo",
                "",
                "The answer is 42\ntrue\n1.250000e+00\n",
                "",
            ),
            (
                "examples/console_error.dodo",
                "",
                "",
                "Could not read the number: text: InvalidDigit position=0\n",
            ),
        ] {
            let exe = work.build(&root.join(source), optimization);
            let output = run(&exe, input.as_bytes());
            assert_eq!(output.stdout, stdout.as_bytes(), "{source}");
            assert_eq!(output.stderr, stderr.as_bytes(), "{source}");
        }
        let exe = work.build(&root.join("examples/console_read_line.dodo"), optimization);
        for input in ["Ada\n", "Ada\r\n", "Ada", "Zoë\n"] {
            let output = run(&exe, input.as_bytes());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("Your name: Hello, {}!\n", input.trim_end())
            );
            assert!(output.stderr.is_empty());
        }
        let output = run(&exe, b"");
        assert_eq!(output.stdout, b"Your name: ");
        assert!(output.stderr.is_empty());
        let output = run(&exe, &[b'x'; 128]);
        assert_eq!(output.stderr, b"The line did not fit in 128 bytes.\n");
        let output = run(&exe, b"\xff\n");
        assert_eq!(output.stderr, b"text: InvalidLead position=0\n");
    }
}

#[test]
fn console_is_embedded_and_only_selects_the_platform_boundary() {
    let work = Workspace::new();
    let source = work.source(
        "console",
        "package app\nimport \"std/console\"\nfn main() {}\n",
    );
    for (target, adapter) in [
        ("x86_64-unknown-linux-gnu", "linux"),
        ("x86_64-pc-windows-msvc", "windows"),
        ("x86_64-w64-windows-gnu", "windows"),
    ] {
        let loaded = package::load_for_target(&source, target).unwrap();
        assert!(
            loaded
                .sources
                .iter()
                .any(|s| s.path.ends_with("std/console.dodo"))
        );
        assert!(
            loaded
                .sources
                .iter()
                .any(|s| s.path.ends_with(format!("std/platform/{adapter}.dodo")))
        );
        assert_eq!(
            package::native_sources(&loaded)
                .iter()
                .map(|s| s.0)
                .collect::<Vec<_>>(),
            ["std/platform/runtime.c"]
        );
        for other in [
            "alloc/",
            "std/fs",
            "std/process",
            "std/thread",
            "std/env",
            "std/net",
            "std/tls",
        ] {
            assert!(
                !loaded.program.imports.iter().any(|s| s.starts_with(other)),
                "console imported {other}"
            );
        }
    }
    for target in [
        "wasm32-unknown-unknown",
        "thumbv6m-none-eabi",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-gnu",
    ] {
        assert!(
            package::load_for_target(&source, target)
                .unwrap_err()
                .contains("unsupported for target")
        );
    }
    let portable = work.source("portable", "package app\nimport \"std/io\"\nimport \"std/fmt\"\nimport \"std/fmt/errors\"\nfn main() {}\n");
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        assert!(
            package::native_sources(&package::load_for_target(&portable, target).unwrap())
                .is_empty()
        );
    }
    // A relocated compiler embeds the adapter and native C source as well.
    let compiler = work
        .0
        .join(format!("installed-dodo{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_dodo"), &compiler).unwrap();
    let relocated = work.source(
        "relocated",
        "package relocated\nimport \"std/console\"\nfn main() -> i32 { match console.println(\"Hello, world!\") { ok(_) => { return 0 } err(_) => { return 1 } } }\n",
    );
    let output = success(
        Command::new(&compiler)
            .current_dir(&work.0)
            .arg("run")
            .arg(&relocated)
            .output()
            .unwrap(),
    );
    assert_eq!(output.stdout, b"Hello, world!\n");
}

#[test]
fn console_windows_objects_and_borrow_rejections() {
    let work = Workspace::new();
    for fixture in ["console_checks", "console_windows_checks"] {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/os/{fixture}.dodo"));
        for target in ["x86_64-pc-windows-msvc", "x86_64-w64-windows-gnu"] {
            for optimization in ["0", "3"] {
                let obj = work.0.join(format!("{target}-{optimization}.obj"));
                success(
                    Command::new(env!("CARGO_BIN_EXE_dodo"))
                        .arg("build")
                        .arg(&source)
                        .args([
                            "--emit",
                            "obj",
                            "--target",
                            target,
                            "-O",
                            optimization,
                            "-o",
                        ])
                        .arg(&obj)
                        .output()
                        .unwrap(),
                );
                assert!(fs::metadata(obj).unwrap().len() > 0);
            }
        }
    }
    for (name, body, diagnostic) in [
        (
            "line_view",
            "fn main() { input := console.stdin()\n storage := [0u8;8]\n match text.Text.new(&storage) { ok(view) => { match input.read_line(&mut storage) { ok(_) => {}, err(_) => {} }\n value := view.as_str() }, err(_) => {} } }",
            "borrow",
        ),
        (
            "unhandled_read",
            "fn main() { input := console.stdin()\n storage := [0u8;8]\n input.read_line(&mut storage) }",
            "result",
        ),
        (
            "private_handle",
            "fn main() { out := console.stdout()\n raw := out.handle }",
            "private",
        ),
        (
            "unhandled",
            "fn main() { console.println(\"hello\") }",
            "result",
        ),
        (
            "close",
            "fn main() { out := console.stdout()\n out.close() }",
            "close",
        ),
        (
            "formatter",
            "fn main() { out := console.stdout()\n f := fmt.Formatter.new::<console.Output>(&mut out)\n core.drop(out)\n match f.string(\"hello\") { ok() => {}, err(_) => {} } }",
            "borrow",
        ),
        (
            "input",
            "fn main() { input := console.stdin()\n storage := [0u8;8]\n match io.BufferedReader.new(&mut input, &mut storage) { ok(buffered) => { storage[0] = 1\n value := buffered.buffered().len }, err(_) => {} } }",
            "borrow",
        ),
        (
            "output",
            "fn main() { out := console.stdout()\n storage := [0u8;8]\n match io.BufferedWriter.new(&mut out, &mut storage) { ok(buffered) => { match out.println(\"hello\") { ok(_) => {}, err(_) => {} }\n match buffered.flush() { ok(_) => {}, err(_) => {} } }, err(_) => {} } }",
            "borrow",
        ),
    ] {
        let source = work.source(name, &format!("package invalid\nimport \"std/console\"\nimport \"std/fmt\"\nimport \"std/io\"\nimport \"std/text\"\n{body}\n"));
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {name}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .to_lowercase()
                .contains(diagnostic),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_closed_nonblocking_and_broken_streams_are_recoverable() {
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::process::CommandExt;
    fn pipe() -> (fs::File, fs::File) {
        let mut fds = [0; 2];
        assert_eq!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) },
            0
        );
        unsafe { (fs::File::from_raw_fd(fds[0]), fs::File::from_raw_fd(fds[1])) }
    }
    let work = Workspace::new();
    for optimization in ["0", "3"] {
        for (name, operation, kind, code, progress) in [
            ("closed", "console.println(\"x\")", "Closed", 9, 0),
            ("broken", "console.println(\"x\")", "BrokenPipe", 32, 0),
            ("blocked", "input.read(&mut bytes)", "WouldBlock", 11, 0),
            (
                "partial",
                "io.write_all(&mut output, &bytes)",
                "WouldBlock",
                11,
                4096,
            ),
        ] {
            let source = work.source(name, &format!(r#"package native_failure
import "std/console"
import "std/io"
fn main() -> i32 {{
    input := console.stdin()
    output := console.stdout()
    bytes := [120u8; 8192]
    match {operation} {{
        ok(_) => {{ return 1 }}
        err(reason) => {{
            if reason.kind == io.ErrorKind.{kind} && reason.code == {code} && reason.transferred == {progress} {{ return 0 }}
            return 2
        }}
    }}
}}
"#));
            let exe = work.build(&source, optimization);
            let (read, write) = pipe();
            let mut read = if name == "broken" {
                drop(read);
                None
            } else {
                Some(read)
            };
            let fd = if name == "blocked" {
                read.as_ref().unwrap().as_raw_fd()
            } else {
                write.as_raw_fd()
            };
            if name == "partial" {
                assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETPIPE_SZ, 4096) }, 4096);
            }
            let closed = name == "closed";
            let destination = if name == "blocked" { 0 } else { 1 };
            let mut command = Command::new(exe);
            unsafe {
                command.pre_exec(move || {
                    // Prove EPIPE is recoverable with the normal fatal SIGPIPE
                    // disposition, regardless of the Rust test runner's disposition.
                    if libc::signal(libc::SIGPIPE, libc::SIG_DFL) == libc::SIG_ERR {
                        return Err(std::io::Error::last_os_error());
                    }
                    if closed {
                        libc::close(destination);
                    } else if libc::dup2(fd, destination) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            success(command.output().unwrap());
            drop(write);
            if name == "partial" {
                let mut bytes = Vec::new();
                read.as_mut().unwrap().read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, vec![b'x'; 4096]);
            }
        }
    }
}

#[cfg(target_os = "windows")]
#[test]
fn windows_absent_standard_streams_and_native_codes() {
    let work = Workspace::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/os/console_windows_checks.dodo");
    for optimization in ["0", "3"] {
        success(
            Command::new(work.build(&source, optimization))
                .output()
                .unwrap(),
        );
    }
}
