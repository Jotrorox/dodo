//! Desktop windows: the X11 protocol code and key tables everywhere, code
//! generation for every hosted target, the C runtime for each window system,
//! and on Linux a real window on a virtual X server driven with xdotool.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("dodo-window-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn build(source: &Path, executable: &Path, optimization: &str) {
    success(
        &Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
            .arg(executable)
            .output()
            .unwrap(),
        &format!("compile {} -O{optimization}", source.display()),
    );
}

#[cfg(unix)]
fn tool(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn protocol_and_key_checks_execute() {
    let scratch = Scratch::new("checks");
    let source = root().join("tests/stdlib/window_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "checks-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        build(&source, &executable, optimization);
        success(
            &Command::new(&executable).output().unwrap(),
            &format!("execute window checks -O{optimization}"),
        );
    }
}

#[test]
fn windows_build_for_every_hosted_target() {
    let scratch = Scratch::new("targets");
    let source = root().join("tests/stdlib/window_events.dodo");
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-pc-windows-gnu",
    ] {
        let object = scratch.0.join(format!("events-{target}.o"));
        success(
            &Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args([
                    "build",
                    source.to_str().unwrap(),
                    "--emit",
                    "obj",
                    "--target",
                    target,
                    "-o",
                ])
                .arg(&object)
                .output()
                .unwrap(),
            &format!("emit the window fixture for {target}"),
        );
        assert!(fs::metadata(object).unwrap().len() > 0);
    }
    // The protocol code needs no operating system.
    let portable = scratch.0.join("portable.dodo");
    fs::write(
        &portable,
        "package main\nimport \"std/window/x11\"\nfn main() {\n    out := [0u8; 64]\n    core.assert_eq(x11.map_window(&mut out, 1), 8usize)\n}\n",
    )
    .unwrap();
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        let object = scratch.0.join(format!("portable-{target}.o"));
        success(
            &Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args([
                    "build",
                    portable.to_str().unwrap(),
                    "--emit",
                    "obj",
                    "--target",
                    target,
                    "-o",
                ])
                .arg(&object)
                .output()
                .unwrap(),
            &format!("emit std/window/x11 for {target}"),
        );
    }
    // Each OS has its own adapter; there is no shared POSIX one, and an
    // adapter cannot be selected for another OS.
    for (import, target, expected) in [
        (
            "std/window/posix",
            "x86_64-unknown-linux-gnu",
            "cannot be used",
        ),
        (
            "std/window/macos",
            "x86_64-unknown-linux-gnu",
            "cannot be used",
        ),
        (
            "std/window/linux",
            "x86_64-pc-windows-msvc",
            "cannot be used",
        ),
        ("std/window/native", "wasm32-unknown-unknown", "unsupported"),
    ] {
        let program = scratch.0.join("adapter.dodo");
        fs::write(
            &program,
            format!("package main\nimport \"{import}\"\nfn main() {{}}\n"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["check", program.to_str().unwrap(), "--target", target])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{import} accepted for {target}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{import} for {target}: {stderr}");
    }
}

#[cfg(unix)]
#[test]
fn window_runtime_compiles_cleanly() {
    let runtime = root().join("stdlib/std/window/runtime.c");
    let mut compilers = vec!["cc"];
    if tool("x86_64-w64-mingw32-gcc") {
        compilers.push("x86_64-w64-mingw32-gcc");
    }
    for compiler in compilers {
        success(
            &Command::new(compiler)
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-fsyntax-only"])
                .arg(&runtime)
                .output()
                .unwrap(),
            &format!("compile runtime.c with {compiler}"),
        );
    }
}

/// A real window needs a window system; Linux CI provides one with Xvfb.
#[cfg(target_os = "linux")]
mod x11 {
    use super::*;
    use std::process::Child;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    #[test]
    fn missing_display_is_reported() {
        let scratch = Scratch::new("no-display");
        let executable = scratch.0.join("events");
        build(
            &root().join("tests/stdlib/window_events.dodo"),
            &executable,
            "0",
        );
        let output = Command::new(&executable)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "error NoDisplay"
        );
    }

    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn xdotool(display: &str, args: &[&str]) -> String {
        let output = Command::new("xdotool")
            .args(args)
            .env("DISPLAY", display)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// The 0xRRGGBB color at (x, y) of a 32-bit XWD screenshot of the root window.
    fn screen_pixel(display: &str, x: usize, y: usize) -> u32 {
        let output = Command::new("xwd")
            .args(["-root", "-silent"])
            .env("DISPLAY", display)
            .output()
            .unwrap();
        success(&output, "xwd");
        let data = output.stdout;
        let field = |index: usize| {
            u32::from_be_bytes(data[index * 4..index * 4 + 4].try_into().unwrap()) as usize
        };
        let (header, byte_order, bits, line, colors) =
            (field(0), field(7), field(11), field(12), field(19));
        assert_eq!((bits, byte_order), (32, 0), "unexpected XWD layout");
        let at = header + colors * 12 + y * line + x * 4;
        u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) & 0xffffff
    }

    /// Whether the X11 tools are installed; CI requires them.
    fn have_window_tools() -> bool {
        let required = std::env::var_os("DODO_REQUIRE_WINDOW_TESTS").is_some();
        let missing: Vec<_> = ["Xvfb", "xdotool", "xwd"]
            .into_iter()
            .filter(|name| !tool(name))
            .collect();
        if !missing.is_empty() {
            assert!(!required, "window tests require {missing:?}");
            eprintln!("skipping: {missing:?} not installed");
            return false;
        }
        true
    }

    /// Starts Xvfb on a display number no other test uses at the same time.
    fn start_xvfb(offset: u32) -> (Server, String) {
        let number = 90 + (std::process::id() * 2 + offset) % 1800;
        let display = format!(":{number}");
        let server = Server(
            Command::new("Xvfb")
                .args([
                    display.as_str(),
                    "-screen",
                    "0",
                    "640x480x24",
                    "-nolisten",
                    "tcp",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
        let started = Instant::now();
        while !socket.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "Xvfb did not start"
            );
            sleep(Duration::from_millis(50));
        }
        (server, display)
    }

    /// Frames keep their pace while events stream in: the mouse moves for
    /// the whole two seconds the fixture animates.
    #[test]
    fn frames_keep_pace_under_xvfb() {
        if !have_window_tools() {
            return;
        }
        let scratch = Scratch::new("frames");
        let executable = scratch.0.join("frames");
        build(
            &root().join("tests/stdlib/window_frames.dodo"),
            &executable,
            "0",
        );
        let (_server, display) = start_xvfb(1);
        let log = scratch.0.join("frames.log");
        let mut app = Command::new(&executable)
            .env("DISPLAY", &display)
            .env_remove("XAUTHORITY")
            .stdout(fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let mut step = 0;
        let status = loop {
            if let Some(status) = app.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(20) {
                let _ = app.kill();
                panic!("the frames program did not exit");
            }
            step += 1;
            let (x, y) = (10 + step * 7 % 140, 10 + step * 5 % 100);
            xdotool(&display, &["mousemove", &x.to_string(), &y.to_string()]);
        };
        let log = fs::read_to_string(&log).unwrap();
        assert!(status.success(), "{status}: {log}");
        let line = log
            .lines()
            .find(|line| line.starts_with("frames "))
            .unwrap_or_else(|| panic!("no summary in:\n{log}"));
        let fields: Vec<u64> = line
            .split(' ')
            .filter_map(|field| field.parse().ok())
            .collect();
        let (frames, events, interval) = (fields[0], fields[1], fields[2]);
        // Xvfb refreshes at 60 Hz, so two seconds hold about 120 frames;
        // the old fixed-wait loop drew none while the mouse moved.
        assert!(events > 0, "no mouse moves arrived: {line}");
        assert!((40..=130).contains(&frames), "{line}");
        assert!((10_000..=25_000).contains(&interval), "{line}");
    }

    #[test]
    fn x11_window_round_trip_under_xvfb() {
        if !have_window_tools() {
            return;
        }
        let scratch = Scratch::new("xvfb");
        let executable = scratch.0.join("events");
        build(
            &root().join("tests/stdlib/window_events.dodo"),
            &executable,
            "0",
        );
        let (_server, display) = start_xvfb(0);

        let log = scratch.0.join("events.log");
        let mut app = Command::new(&executable)
            .env("DISPLAY", &display)
            .env_remove("XAUTHORITY")
            .stdout(fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let window = loop {
            let found = xdotool(&display, &["search", "--name", "^Events$"]);
            if !found.is_empty() && fs::read_to_string(&log).unwrap().contains("drawn") {
                break found.lines().next().unwrap().to_owned();
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the window did not appear"
            );
            sleep(Duration::from_millis(50));
        };
        // Without a window manager the window sits at the root's origin.
        assert_eq!(screen_pixel(&display, 20, 20), 0xff0000);
        assert_eq!(screen_pixel(&display, 90, 20), 0x00ff00);
        assert_eq!(screen_pixel(&display, 150, 20), 0x0000ff);
        assert_eq!(screen_pixel(&display, 300, 200), 0x203040);

        xdotool(&display, &["mousemove", "50", "60", "click", "1"]);
        xdotool(&display, &["key", "a", "shift+b", "t", "p", "d"]);
        xdotool(&display, &["click", "5"]);
        sleep(Duration::from_millis(300));
        assert_eq!(xdotool(&display, &["getwindowname", &window]), "Renamed");
        assert_eq!(screen_pixel(&display, 210, 110), 0xff00ff);
        assert_eq!(screen_pixel(&display, 260, 160), 0x00ffff);
        xdotool(&display, &["windowsize", &window, "400", "300"]);
        sleep(Duration::from_millis(300));
        xdotool(&display, &["key", "Escape"]);

        let started = Instant::now();
        let status = loop {
            if let Some(status) = app.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(10) {
                let _ = app.kill();
                panic!(
                    "the window program did not exit: {}",
                    fs::read_to_string(&log).unwrap()
                );
            }
            sleep(Duration::from_millis(50));
        };
        let log = fs::read_to_string(&log).unwrap();
        assert!(status.success(), "{status}: {log}");
        let lines: Vec<&str> = log.lines().collect();
        let position = |line: &str| {
            lines
                .iter()
                .position(|candidate| *candidate == line)
                .unwrap_or_else(|| panic!("missing {line:?} in:\n{log}"))
        };
        // Events arrive in order, and typing reports physical keys and text.
        let expected = [
            "open 320 240",
            "drawn",
            "move 50 60",
            "button true 50 60",
            "button false 50 60",
            "key A true false false",
            "text 97",
            "key B true false true",
            "text 66",
            "key T true false false",
            "painted",
            "presented",
            "scroll 0 120",
            "resize 400 300",
            "key Escape true false false",
            "closed",
        ];
        let mut previous = 0;
        for line in expected {
            let index = position(line);
            assert!(index >= previous, "{line:?} out of order in:\n{log}");
            previous = index;
        }
    }
}
