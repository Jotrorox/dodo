//! Desktop windows: the X11 protocol code and key tables everywhere, code
//! generation for every hosted target, the C runtime for each window system,
//! and on Linux real windows on a virtual X server driven with xdotool: one
//! with a pixel buffer, GPU windows that OpenGL (Mesa's llvmpipe) and Vulkan
//! (lavapipe) present into, and the clipboard, cursor and size controls,
//! with xclip as the other side of the clipboard. On macOS the GPU window
//! and desktop fixtures run when DODO_DESKTOP_TESTS is set, since they open
//! windows on the desktop.
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
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-pc-windows-gnu",
    ] {
        for fixture in [
            "window_events",
            "window_multi",
            "window_gpu",
            "window_desktop",
        ] {
            let source = root().join(format!("tests/stdlib/{fixture}.dodo"));
            let object = scratch.0.join(format!("{fixture}-{target}.o"));
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
                &format!("emit {fixture} for {target}"),
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
    // The protocol code and the native handle type need no operating
    // system, so GPU packages can name handles on any target.
    let portable = scratch.0.join("portable.dodo");
    fs::write(
        &portable,
        "package main\nimport \"std/window/handle\"\nimport \"std/window/x11\"\nfn main() {\n    out := [0u8; 64]\n    core.assert_eq(x11.map_window(&mut out, 1), 8usize)\n    native := handle.NativeHandle.Win32(1, 2)\n    core.assert_eq(handle.kind_name(&native).len, 5usize)\n}\n",
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

/// A GPU window on the macOS desktop: its AppKit handle, its CAMetalLayer,
/// and no pixel buffer. Opt-in, because it shows a window.
#[cfg(target_os = "macos")]
#[test]
fn gpu_window_on_the_macos_desktop() {
    if std::env::var_os("DODO_DESKTOP_TESTS").is_none() {
        eprintln!("skipping: set DODO_DESKTOP_TESTS to open desktop windows");
        return;
    }
    let scratch = Scratch::new("macos-gpu");
    let executable = scratch.0.join("gpu");
    build(
        &root().join("tests/stdlib/window_gpu.dodo"),
        &executable,
        "0",
    );
    let output = Command::new(&executable).output().unwrap();
    success(&output, "run the GPU window fixture");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("native AppKit "), "{stdout}");
    assert!(stdout.ends_with("passed\n"), "{stdout}");
}

/// The clipboard, every cursor shape, set_size with its limits, and
/// fullscreen on the macOS desktop. Opt-in, because it shows a window, goes
/// fullscreen briefly and uses the clipboard; text on the clipboard is put
/// back afterwards.
#[cfg(target_os = "macos")]
#[test]
fn desktop_basics_on_the_macos_desktop() {
    if std::env::var_os("DODO_DESKTOP_TESTS").is_none() {
        eprintln!("skipping: set DODO_DESKTOP_TESTS to open desktop windows");
        return;
    }
    let scratch = Scratch::new("macos-desktop");
    let executable = scratch.0.join("desktop");
    build(
        &root().join("tests/stdlib/window_desktop.dodo"),
        &executable,
        "0",
    );
    let saved = Command::new("pbpaste").output().unwrap().stdout;
    let output = Command::new(&executable).output().unwrap();
    let mut restore = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    restore.stdin.take().unwrap().write_all(&saved).unwrap();
    restore.wait().unwrap();
    success(&output, "run the desktop fixture");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.ends_with("passed\n"), "{stdout}");
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
                    // Without it the server resets when its last client
                    // disconnects, and a program reconnecting with a new
                    // App can catch it mid-reset.
                    "-noreset",
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

    /// Several windows on one App: each draws its own pixels and gets its
    /// own events, a window opened later works like the first, and closing
    /// one leaves the others running.
    #[test]
    fn multiple_windows_under_xvfb() {
        if !have_window_tools() {
            return;
        }
        let scratch = Scratch::new("multi");
        let executable = scratch.0.join("multi");
        build(
            &root().join("tests/stdlib/window_multi.dodo"),
            &executable,
            "0",
        );
        let (_server, display) = start_xvfb(2);
        let log = scratch.0.join("multi.log");
        let mut app = Command::new(&executable)
            .env("DISPLAY", &display)
            .env_remove("XAUTHORITY")
            .stdout(fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while !fs::read_to_string(&log).unwrap().contains("drawn") {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the windows did not appear"
            );
            sleep(Duration::from_millis(50));
        }
        sleep(Duration::from_millis(300));
        assert!(!xdotool(&display, &["search", "--name", "^Main$"]).is_empty());
        assert!(!xdotool(&display, &["search", "--name", "^Tools$"]).is_empty());
        // Without a window manager both sit at the root's origin, Tools on
        // top of the larger Main.
        assert_eq!(screen_pixel(&display, 20, 20), 0x00ff00);
        assert_eq!(screen_pixel(&display, 200, 150), 0xff0000);

        // The focus follows the pointer, so keys go to the window below it.
        xdotool(&display, &["mousemove", "50", "40", "click", "1"]);
        xdotool(&display, &["key", "a"]);
        xdotool(&display, &["mousemove", "200", "150", "click", "1"]);
        xdotool(&display, &["key", "b", "n"]);
        sleep(Duration::from_millis(300));
        assert!(!xdotool(&display, &["search", "--name", "^Dialog$"]).is_empty());
        assert_eq!(screen_pixel(&display, 20, 20), 0x0000ff);
        xdotool(&display, &["mousemove", "20", "20", "key", "w"]);
        sleep(Duration::from_millis(200));
        xdotool(&display, &["mousemove", "50", "70", "key", "w"]);
        sleep(Duration::from_millis(300));
        assert!(xdotool(&display, &["search", "--name", "^Tools$"]).is_empty());
        assert_eq!(screen_pixel(&display, 20, 20), 0xff0000);
        xdotool(&display, &["mousemove", "200", "150", "key", "Escape"]);

        let started = Instant::now();
        let status = loop {
            if let Some(status) = app.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(10) {
                let _ = app.kill();
                panic!(
                    "the windows program did not exit: {}",
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
        let expected = [
            "second app AlreadyOpen",
            "zero size InvalidInput",
            "huge size InvalidInput",
            "open true 320 240 120 90",
            "drawn",
            "button tools true 50 40",
            "key tools A true",
            "text tools 97",
            "button main true 200 150",
            "key main B true",
            "text main 98",
            "key main N true",
            "dialog true",
            "key dialog W true",
            "dialog closed",
            "key tools W true",
            "tools closed",
            "key main Escape true",
            "stale Closed",
            "stale size 0 0",
            "reopened true",
        ];
        let mut previous = 0;
        for line in expected {
            let index = position(line);
            assert!(index >= previous, "{line:?} out of order in:\n{log}");
            previous = index;
        }
        // Both windows got the frame they asked for.
        position("frame main");
        position("frame tools");
    }

    /// What xclip reads from the clipboard.
    fn xclip_out(display: &str, args: &[&str]) -> Vec<u8> {
        let output = Command::new("xclip")
            .args(["-selection", "clipboard", "-o"])
            .args(args)
            .env("DISPLAY", display)
            .output()
            .unwrap();
        success(&output, "xclip -o");
        output.stdout
    }

    /// Copy `data` with xclip, which keeps serving it from a background
    /// process holding no pipe of ours. That process takes the selection
    /// after the foreground one exits, so give it a moment.
    fn xclip_in(display: &str, args: &[&str], data: &[u8]) {
        use std::io::Write;
        let mut child = Command::new("xclip")
            .args(["-selection", "clipboard", "-i"])
            .args(args)
            .env("DISPLAY", display)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(data).unwrap();
        assert!(child.wait().unwrap().success(), "xclip -i");
        sleep(Duration::from_millis(500));
    }

    /// The clipboard, cursor shapes, set_size with its limits and fullscreen
    /// on a real window. Without a window manager, fullscreen covers the
    /// screen by itself. xclip is the other client: it reads what the
    /// fixture copied, including a text larger than one request, and copies
    /// a short UTF-8 text, a 5 MB text it sends in increments (INCR) and a
    /// Latin-1 STRING for the fixture to paste.
    #[test]
    fn desktop_basics_under_xvfb() {
        if !have_window_tools() {
            return;
        }
        let interop = tool("xclip");
        if !interop {
            assert!(
                std::env::var_os("DODO_REQUIRE_WINDOW_TESTS").is_none(),
                "window tests require xclip"
            );
            eprintln!("xclip not installed: checking without another client");
        }
        let scratch = Scratch::new("desktop");
        let executable = scratch.0.join("desktop");
        build(
            &root().join("tests/stdlib/window_desktop.dodo"),
            &executable,
            "0",
        );
        let (_server, display) = start_xvfb(5);
        let log = scratch.0.join("desktop.log");
        let args: &[&str] = if interop { &["interop"] } else { &[] };
        let (app, copied) = launch(&executable, args, &display, &log, "copied");
        if !interop {
            let (status, log) = finish(app, &log);
            assert!(status.success(), "{status}: {log}");
            assert!(log.ends_with("passed\n"), "{log}");
            return;
        }
        assert!(copied, "{}", fs::read_to_string(&log).unwrap());
        let targets = String::from_utf8(xclip_out(&display, &["-t", "TARGETS"])).unwrap();
        assert_eq!(
            targets.split_whitespace().collect::<Vec<_>>(),
            ["TARGETS", "UTF8_STRING", "TEXT"]
        );
        assert_eq!(xclip_out(&display, &[]), "from dodo ✓".as_bytes());

        let wait_for = |line: &str| {
            let started = Instant::now();
            while !fs::read_to_string(&log)
                .unwrap()
                .lines()
                .any(|candidate| candidate == line)
            {
                assert!(
                    started.elapsed() < Duration::from_secs(20),
                    "no {line:?} in:\n{}",
                    fs::read_to_string(&log).unwrap()
                );
                sleep(Duration::from_millis(50));
            }
        };
        // Keys go to the window under the pointer.
        xdotool(&display, &["mousemove", "20", "20", "click", "1"]);
        xdotool(&display, &["key", "t", "h", "a"]);
        xdotool(&display, &["key", "l"]);
        wait_for("copied large");
        let large = xclip_out(&display, &[]);
        assert_eq!(large.len(), 300_000);
        assert!(
            large
                .iter()
                .enumerate()
                .all(|(i, byte)| *byte == b'a' + (i % 26) as u8)
        );

        xclip_in(&display, &[], "from xclip é".as_bytes());
        xdotool(&display, &["key", "v"]);
        wait_for("pasted from xclip é");
        xclip_in(&display, &[], &vec![b'x'; 5_000_000]);
        xdotool(&display, &["key", "v"]);
        wait_for("pasted 5000000 bytes 120 120");
        xclip_in(&display, &["-t", "STRING"], b"caf\xe9");
        xdotool(&display, &["key", "v"]);
        wait_for("pasted café");
        xdotool(&display, &["key", "Escape"]);

        let (status, log) = finish(app, &log);
        assert!(status.success(), "{status}: {log}");
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(
            lines.iter().filter(|line| **line == "cursor").count(),
            3,
            "{log}"
        );
        for line in [
            "clipboard round trip",
            "cursors set",
            "resized 300 200",
            "limited 360 230",
            "limits removed",
            "fullscreen",
            "windowed 310 210",
            "passed",
        ] {
            assert!(lines.contains(&line), "missing {line:?} in:\n{log}");
        }
    }

    /// Starts `executable` with `args` on `display`, logging stdout to
    /// `log`, and waits until the log contains `marker` or the program
    /// exits. Returns the child and whether the marker appeared.
    fn launch(
        executable: &Path,
        args: &[&str],
        display: &str,
        log: &Path,
        marker: &str,
    ) -> (Child, bool) {
        let mut app = Command::new(executable)
            .args(args)
            .env("DISPLAY", display)
            .env_remove("XAUTHORITY")
            .stdout(fs::File::create(log).unwrap())
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if fs::read_to_string(log).unwrap().contains(marker) {
                return (app, true);
            }
            if app.try_wait().unwrap().is_some() {
                return (app, false);
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "{} did not print {marker:?}: {}",
                executable.display(),
                fs::read_to_string(log).unwrap()
            );
            sleep(Duration::from_millis(50));
        }
    }

    fn finish(mut app: Child, log: &Path) -> (std::process::ExitStatus, String) {
        let started = Instant::now();
        let status = loop {
            if let Some(status) = app.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(20) {
                let _ = app.kill();
                panic!(
                    "the program did not exit: {}",
                    fs::read_to_string(log).unwrap()
                );
            }
            sleep(Duration::from_millis(50));
        };
        (status, fs::read_to_string(log).unwrap())
    }

    /// A window without a pixel buffer hands out the X11 window and a libxcb
    /// connection; the id is the one the X server reports.
    #[test]
    fn gpu_window_hands_out_an_xcb_handle_under_xvfb() {
        if !have_window_tools() {
            return;
        }
        let scratch = Scratch::new("xvfb-gpu");
        let executable = scratch.0.join("gpu");
        build(
            &root().join("tests/stdlib/window_gpu.dodo"),
            &executable,
            "0",
        );
        let (_server, display) = start_xvfb(2);
        let log = scratch.0.join("gpu.log");
        let (app, shown) = launch(&executable, &[], &display, &log, "native ");
        assert!(shown, "{}", fs::read_to_string(&log).unwrap());
        let windows = xdotool(&display, &["search", "--name", "^GPU$"]);
        let (status, log) = finish(app, &log);
        assert!(status.success(), "{status}: {log}");
        let id: u64 = log
            .lines()
            .find_map(|line| line.strip_prefix("native Xcb "))
            .unwrap_or_else(|| panic!("no Xcb handle in:\n{log}"))
            .parse()
            .unwrap();
        assert!(
            windows.lines().any(|line| line.parse() == Ok(id)),
            "window {id} not among {windows:?}"
        );
        assert!(log.ends_with("passed\n"), "{log}");
    }

    /// OpenGL draws straight into the window: what the context swapped in
    /// is what the X server shows.
    #[test]
    fn opengl_window_presents_under_xvfb() {
        if !have_window_tools() {
            return;
        }
        let required = std::env::var_os("DODO_REQUIRE_OPENGL_TESTS").is_some();
        let scratch = Scratch::new("xvfb-opengl");
        let executable = scratch.0.join("opengl");
        build(
            &root().join("tests/gpu/opengl_window_checks.dodo"),
            &executable,
            "0",
        );
        let (_server, display) = start_xvfb(3);
        let log = scratch.0.join("opengl.log");
        let (app, shown) = launch(&executable, &[], &display, &log, "shown");
        if !shown {
            let (status, log) = finish(app, &log);
            assert!(status.success(), "{status}: {log}");
            let reason = log
                .strip_prefix("unavailable\n")
                .unwrap_or_else(|| panic!("{log}"));
            assert!(!required, "OpenGL is required but unavailable: {reason}");
            eprintln!("skipping: no OpenGL window ({})", reason.trim());
            return;
        }
        sleep(Duration::from_millis(300));
        // Without a window manager the window sits at the root's origin.
        assert_eq!(screen_pixel(&display, 2, 2), 0x336699);
        assert_eq!(screen_pixel(&display, 80, 60), 0xff0000);
        xdotool(&display, &["mousemove", "40", "40", "key", "Escape"]);
        let (status, log) = finish(app, &log);
        assert!(status.success(), "{status}: {log}");
        assert!(log.ends_with("passed\n"), "{log}");
    }

    /// Vulkan presents into the window through VK_KHR_xcb_surface, and
    /// rebuilds its swapchain when the window is resized.
    #[test]
    fn vulkan_window_presents_under_xvfb() {
        if !have_window_tools() {
            return;
        }
        let required = std::env::var_os("DODO_REQUIRE_VULKAN_TESTS").is_some();
        let scratch = Scratch::new("xvfb-vulkan");
        let executable = scratch.0.join("vulkan");
        build(
            &root().join("examples/vulkan_window.dodo"),
            &executable,
            "0",
        );
        let (_server, display) = start_xvfb(4);
        let log = scratch.0.join("vulkan.log");
        let (app, shown) = launch(&executable, &["150"], &display, &log, "presented");
        if !shown {
            let (status, log) = finish(app, &log);
            // No loader (2), or a loader without a driver that presents.
            let missing = status.code() == Some(2)
                || (status.code() == Some(1)
                    && [
                        "ERROR_INCOMPATIBLE_DRIVER",
                        "ERROR_INITIALIZATION_FAILED",
                        "ERROR_EXTENSION_NOT_PRESENT",
                    ]
                    .iter()
                    .any(|name| log.contains(name)));
            assert!(missing, "{status}: {log}");
            assert!(!required, "Vulkan is required but unavailable: {log}");
            eprintln!("skipping: no Vulkan driver ({})", log.trim());
            return;
        }
        sleep(Duration::from_millis(300));
        // The 480x320 window: background in the corner, and the spinning
        // triangle always covers the centre.
        let background = 0x1c2130;
        assert_eq!(screen_pixel(&display, 3, 3), background);
        let centre = screen_pixel(&display, 240, 160);
        assert!(centre != background && centre != 0, "centre {centre:06x}");
        let window = xdotool(&display, &["search", "--class", "Dodo"]);
        let window = window.lines().next().unwrap().to_owned();
        xdotool(&display, &["windowsize", &window, "600", "200"]);
        sleep(Duration::from_millis(500));
        // The rebuilt swapchain covers the wider window.
        assert_eq!(screen_pixel(&display, 590, 190), background);
        let (status, log) = finish(app, &log);
        assert!(status.success(), "{status}: {log}");
        assert!(log.contains("150 frames"), "{log}");
    }
}
