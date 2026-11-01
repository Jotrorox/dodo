//! The Lua 5.4 conformance corpus (`tests/lua`) run through the standalone
//! interpreter (`examples/lua`) built with this compiler, and the embedding
//! example (`examples/lua_embed`).
//!
//! Each `NAME.lua` must reproduce the exit status, stdout and stderr that
//! PUC Lua 5.4.9 recorded in `NAME.expected`. This is a Rust port of
//! `scripts/test_lua.py check` (directives, environment, stderr
//! normalization and the expected-file format are described in
//! tests/lua/README.md), so `cargo test` needs no Python.
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const COMPILER: &str = env!("CARGO_BIN_EXE_dodo");
const TIMEOUT: Duration = Duration::from_secs(60);
/// Environment variables that change interpreter behaviour.
const LUA_ENV_VARS: [&str; 6] = [
    "LUA_INIT",
    "LUA_INIT_5_4",
    "LUA_PATH",
    "LUA_PATH_5_4",
    "LUA_CPATH",
    "LUA_CPATH_5_4",
];
/// Variables the corpus may read, and a fixed locale and time zone.
const TEST_ENV: [(&str, &str); 4] = [
    ("LUA_TEST_VAR", "hello from env"),
    ("LC_ALL", "C"),
    ("LANG", "C"),
    ("TZ", "UTC"),
];

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-lua-corpus-{}-{}",
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

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build(project: &Path, exe: &Path) {
    let output = Command::new(COMPILER)
        .arg("build")
        .arg(project)
        .arg("-o")
        .arg(exe)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "building {} failed:\n{}",
        project.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

// --- Directives ------------------------------------------------------------

#[derive(Debug, Default, PartialEq)]
struct Directives {
    opts: Vec<String>,
    args: Vec<String>,
    cmd: Option<Vec<String>>,
    stdin: Option<String>,
    stderr: StderrMode,
}

#[derive(Debug, Default, PartialEq, Clone, Copy)]
enum StderrMode {
    /// Compare stderr up to the first `stack traceback:` line.
    #[default]
    Message,
    Full,
    Ignore,
}

/// POSIX shell word splitting as done by Python's `shlex.split`: blanks
/// separate words, single quotes are literal, double quotes allow `\"`,
/// `\\`, `\$` and `` \` `` escapes, and a backslash outside quotes escapes
/// the next character.
fn shell_split(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    word.push(q);
                }
            }
            '"' => {
                in_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&e @ ('"' | '\\' | '$' | '`')) => {
                                word.push(e);
                                chars.next();
                            }
                            _ => word.push('\\'),
                        },
                        _ => word.push(q),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(e) = chars.next() {
                    word.push(e);
                }
            }
            _ => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// `--! key: value` lines in the leading comment block of a script.
fn parse_directives(path: &Path) -> Directives {
    let text = String::from_utf8_lossy(&fs::read(path).unwrap()).into_owned();
    let mut d = Directives::default();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if (index == 0 && line.starts_with('#')) || line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = directive(line) else {
            if line.starts_with("--") && !line.starts_with("--[[") {
                continue; // directives may follow the header comment
            }
            break;
        };
        match key {
            "opts" => d.opts = shell_split(value),
            "args" => d.args = shell_split(value),
            "cmd" => d.cmd = Some(shell_split(value)),
            "stdin" => d.stdin = Some(value.to_string()),
            "stderr" => {
                d.stderr = match value {
                    "message" => StderrMode::Message,
                    "full" => StderrMode::Full,
                    "ignore" => StderrMode::Ignore,
                    _ => panic!("{}: bad stderr directive {value:?}", path.display()),
                }
            }
            _ => panic!("{}: unknown directive {key:?}", path.display()),
        }
    }
    d
}

/// Split `--! key: value` into its key and trimmed value.
fn directive(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("--!")?.trim_start();
    let key_end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if key_end == 0 {
        return None;
    }
    let (key, rest) = rest.split_at(key_end);
    let value = rest.trim_start().strip_prefix(':')?;
    Some((key, value.trim()))
}

// --- Running ---------------------------------------------------------------

struct Run {
    /// None: killed after the time limit.
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn kill(child: &mut Child) {
    // The child leads its own process group: also stop commands it started.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

fn run(argv: &[String], cwd: &Path, stdin: Vec<u8>) -> Run {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in LUA_ENV_VARS {
        command.env_remove(name);
    }
    command.envs(TEST_ENV);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = thread::spawn(move || {
        // The script may exit without reading its input.
        let _ = input.write_all(&stdin);
    });
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let err_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + TIMEOUT;
    let code = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status.code().unwrap_or(-1));
        }
        if Instant::now() >= deadline {
            kill(&mut child);
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(5));
    };
    let _ = writer.join();
    Run {
        code,
        stdout: out_reader.join().unwrap(),
        stderr: err_reader.join().unwrap(),
    }
}

fn run_script(lua: &Path, script: &Path) -> (Run, StderrMode) {
    let d = parse_directives(script);
    let dir = script.parent().unwrap();
    let name = script.file_name().unwrap().to_str().unwrap();
    let stdin = match &d.stdin {
        Some(file) => fs::read(dir.join(file.replace("{script}", name))).unwrap(),
        None => Vec::new(),
    };
    let mut argv = vec![lua.to_str().unwrap().to_string()];
    match &d.cmd {
        Some(cmd) => argv.extend(cmd.iter().map(|a| a.replace("{script}", name))),
        None => {
            argv.extend(d.opts.iter().cloned());
            argv.push(name.to_string());
            argv.extend(d.args.iter().cloned());
        }
    }
    (run(&argv, dir, stdin), d.stderr)
}

fn replace_all(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if !needle.is_empty() && haystack[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// The program name prefixed to messages becomes `lua:`, other occurrences
/// of the interpreter path become `lua`, and in message mode everything from
/// the first `stack traceback:` line on is dropped.
fn normalize_stderr(text: &[u8], lua: &Path, mode: StderrMode) -> Vec<u8> {
    if mode == StderrMode::Ignore {
        return Vec::new();
    }
    let full = lua.to_str().unwrap().as_bytes();
    let base = lua.file_name().unwrap().to_str().unwrap().as_bytes();
    let mut names = [full, base];
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let mut out = Vec::new();
    for raw in text.split_inclusive(|&b| b == b'\n') {
        let mut line = raw.to_vec();
        for name in names {
            if line.starts_with(name) && line[name.len()..].starts_with(b": ") {
                let mut renamed = b"lua: ".to_vec();
                renamed.extend_from_slice(&line[name.len() + 2..]);
                line = renamed;
                break;
            }
        }
        line = replace_all(&line, full, b"lua");
        let trimmed: &[u8] = {
            let mut end = line.len();
            while end > 0 && (line[end - 1] == b'\n' || line[end - 1] == b'\r') {
                end -= 1;
            }
            &line[..end]
        };
        if mode == StderrMode::Message && trimmed == b"stack traceback:" {
            break;
        }
        out.extend_from_slice(&line);
    }
    out
}

// --- Expected files --------------------------------------------------------

struct Expected {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// `@@exit N`, then `@@stdout` and optionally `@@stderr` sections; a header
/// ending in ` noeol` means the stream had no final newline.
fn parse_expected(path: &Path) -> Expected {
    let data = fs::read(path).unwrap();
    let mut code = 0;
    let mut sections: BTreeMap<&str, (Vec<u8>, bool)> = BTreeMap::new();
    let mut current: Option<&str> = None;
    let body = data.strip_suffix(b"\n").unwrap_or(&data);
    for line in body.split(|&b| b == b'\n') {
        if let Some(header) = line.strip_prefix(b"@@") {
            let header = String::from_utf8_lossy(header);
            let mut parts = header.splitn(2, ' ');
            let name = parts.next().unwrap();
            let arg = parts.next();
            match name {
                "exit" => {
                    code = arg.unwrap().parse().unwrap();
                    current = None;
                    continue;
                }
                "stdout" | "stderr" => {
                    let name = if name == "stdout" { "stdout" } else { "stderr" };
                    sections.entry(name).or_default().1 = arg == Some("noeol");
                    current = Some(name);
                    continue;
                }
                _ => {}
            }
        }
        let section =
            current.unwrap_or_else(|| panic!("{}: content outside a section", path.display()));
        let text = &mut sections.entry(section).or_default().0;
        text.extend_from_slice(line);
        text.push(b'\n');
    }
    let mut take = |name: &str| {
        let (mut text, noeol) = sections.remove(name).unwrap_or_default();
        if noeol && text.ends_with(b"\n") {
            text.pop();
        }
        text
    };
    Expected {
        code,
        stdout: take("stdout"),
        stderr: take("stderr"),
    }
}

fn describe(code: Option<i32>) -> String {
    match code {
        None => "timeout".to_string(),
        Some(c) => format!("exit {c}"),
    }
}

/// A short line-based diff: the first differing line with context.
fn first_difference(stream: &str, want: &[u8], got: &[u8]) -> String {
    let want = String::from_utf8_lossy(want);
    let got = String::from_utf8_lossy(got);
    let w: Vec<&str> = want.split_inclusive('\n').collect();
    let g: Vec<&str> = got.split_inclusive('\n').collect();
    let first = w.iter().zip(&g).take_while(|(a, b)| a == b).count();
    let from = first.saturating_sub(2);
    let mut report = format!("{stream} differs at line {}:\n", first + 1);
    for line in w.iter().skip(from).take(first - from + 6) {
        report.push_str(&format!("  - {}", line.trim_end_matches('\n')));
        report.push('\n');
    }
    for line in g.iter().skip(from).take(first - from + 6) {
        report.push_str(&format!("  + {}", line.trim_end_matches('\n')));
        report.push('\n');
    }
    report
}

fn check_script(lua: &Path, script: &Path) -> Option<String> {
    let want = parse_expected(&script.with_extension("expected"));
    let (got, mode) = run_script(lua, script);
    let stderr = normalize_stderr(&got.stderr, lua, mode);
    let mut problems = Vec::new();
    if got.code != Some(want.code) {
        problems.push(format!(
            "expected: {}, lua: {}",
            describe(Some(want.code)),
            describe(got.code)
        ));
    }
    if got.stdout != want.stdout {
        problems.push(first_difference("stdout", &want.stdout, &got.stdout));
    }
    if mode != StderrMode::Ignore && stderr != want.stderr {
        problems.push(first_difference("stderr", &want.stderr, &stderr));
    }
    if problems.is_empty() {
        None
    } else {
        Some(problems.join("\n"))
    }
}

fn corpus_scripts() -> Vec<PathBuf> {
    let mut scripts: Vec<PathBuf> = fs::read_dir(root().join("tests/lua"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "lua"))
        .collect();
    scripts.sort();
    scripts
}

// --- Tests -----------------------------------------------------------------

#[test]
fn harness_parses_directives_and_expected_files() {
    assert_eq!(
        shell_split(r#"-e "x = 1" -ex=x+10 'a b' "" c\ d "q\"x" -"#),
        ["-e", "x = 1", "-ex=x+10", "a b", "", "c d", "q\"x", "-"]
    );
    assert_eq!(shell_split("  "), Vec::<String>::new());
    assert_eq!(directive("--! opts: -W  \n"), Some(("opts", "-W")));
    assert_eq!(directive("--!cmd:\n"), Some(("cmd", "")));
    assert_eq!(directive("-- plain comment\n"), None);

    let ws = Workspace::new();
    let script = ws.0.join("t.lua");
    fs::write(
        &script,
        "#!/usr/bin/env lua\n-- header\n--! args: one \"two words\"\n\n--! stderr: full\nprint(1)\n--! stdin: ignored\n",
    )
    .unwrap();
    let d = parse_directives(&script);
    assert_eq!(d.args, ["one", "two words"]);
    assert_eq!(d.stderr, StderrMode::Full);
    assert_eq!(d.stdin, None);

    let expected = ws.0.join("t.expected");
    fs::write(
        &expected,
        "@@exit 1\n@@stdout\na\n\n@@stderr noeol\nlua: t.lua:1: boom\n",
    )
    .unwrap();
    let e = parse_expected(&expected);
    assert_eq!(e.code, 1);
    assert_eq!(e.stdout, b"a\n\n");
    assert_eq!(e.stderr, b"lua: t.lua:1: boom");

    let lua = Path::new("/x/build/lua");
    let stderr = b"/x/build/lua: t.lua:1: boom\nusage: /x/build/lua [options]\nstack traceback:\n\t[C]: in ?\n";
    assert_eq!(
        normalize_stderr(stderr, lua, StderrMode::Message),
        b"lua: t.lua:1: boom\nusage: lua [options]\n"
    );
    assert_eq!(
        normalize_stderr(stderr, lua, StderrMode::Full),
        b"lua: t.lua:1: boom\nusage: lua [options]\nstack traceback:\n\t[C]: in ?\n"
    );
}

// The expected files record the reference on Linux and macOS: io.popen and
// os.execute run /bin/sh commands, and Windows translates line endings.
#[test]
#[cfg_attr(windows, ignore = "the corpus records POSIX behaviour")]
fn corpus_matches_reference_interpreter() {
    let ws = Workspace::new();
    let lua = ws.0.join(format!("lua{}", std::env::consts::EXE_SUFFIX));
    build(&root().join("examples/lua"), &lua);
    let scripts = corpus_scripts();
    assert!(scripts.len() > 100, "tests/lua looks incomplete");
    let mut failures = Vec::new();
    for script in &scripts {
        if let Some(problem) = check_script(&lua, script) {
            let name = script.file_name().unwrap().to_string_lossy().into_owned();
            failures.push(format!("==== {name} ====\n{problem}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} corpus scripts failed (python3 scripts/test_lua.py check shows full diffs):\n{}",
        failures.len(),
        scripts.len(),
        failures.join("\n")
    );
}

#[test]
fn embedding_example_runs() {
    let ws = Workspace::new();
    let exe =
        ws.0.join(format!("lua_embed{}", std::env::consts::EXE_SUFFIX));
    build(&root().join("examples/lua_embed"), &exe);
    let output: Output = Command::new(&exe).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "count: 4\nmean: 40.1875\nboiler is 72.5 C (hot)\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: [string \"return clamp(1, 5, 2)\"]:1: empty range\n"
    );
}
