//! Manifest-free test discovery and hosted process isolation.
use super::{Action, Args, Emit, Parsed, TempDir, compile};
use dodoc::ast::{Function, Type};
use dodoc::lexer::{self, TokenKind};
use dodoc::{codegen, package, parser, sema};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const HELP: &str = r#"Usage: dodo test [FILE|DIRECTORY] [OPTIONS]

With no path, recursively scan the current folder. Write @test fn name() {}
or fn test_name() {} in any .dodo file. Tests take no arguments and return void.
Use assert(condition), assert_eq(left, right), or assert_ne(left, right), each
with an optional string message. No imports or main function are needed.

Discovery:
  Inline tests load their file and imports, just like dodo run.
  *_test.dodo and *.test.dodo load their folder as a package, including helpers.
  Markdown fences marked `dodo test` contain executable documentation examples.
  Hidden paths, build, target, dist, node_modules, vendor, and symlinks are skipped.

Options:
      --list                List matching tests and locations without compiling
      --filter TEXT         Match a test name or path (substring; repeatable OR)
      --exact               Match filters against the whole name or path::name
      --skip TEXT           Exclude names containing TEXT; repeatable
      --ignored             Run only tests marked @ignore("reason")
      --include-ignored     Also run ignored tests
      --show-output         Show captured stdout/stderr for passing tests too
      --fail-fast           Stop after the first build or test failure
      --timeout SECONDS     Per-test execution deadline (default: 30; 0 disables)
      --doc                 Discover only executable Markdown examples
      --no-doc              Discover only Dodo source tests
      --allow-empty         Succeed when no tests match (default: exit 1)
  -O, --opt-level LEVEL      0, 1, 2, 3 (default: 0)
      --cpu NAME            Target CPU (default: generic)
      --features LIST       LLVM target features
      --linker PATH         C linker driver (default: DODO_CC or cc)
      --link-arg ARG        Additional linker argument; repeatable
  -h, --help                Show this help

Every test runs in a fresh process with the caller's working directory.
Output is captured and shown on failure, along with source locations. A trap
fails only its test. Exit status: 0 = success; 1 = failures, errors, or no matches.

Examples:
  dodo test
  dodo test --list
  dodo test --filter addition
  dodo test tests --include-ignored -O 3
  dodo test docs --doc
"#;

pub struct TestArgs {
    compiler: Args,
    input: PathBuf,
    list: bool,
    filters: Vec<String>,
    skips: Vec<String>,
    exact: bool,
    ignored: bool,
    include_ignored: bool,
    show_output: bool,
    fail_fast: bool,
    timeout: Duration,
    doc: bool,
    no_doc: bool,
    allow_empty: bool,
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut args = args.into_iter();
    let mut result = TestArgs {
        compiler: Args {
            action: Action::Build,
            input: PathBuf::new(),
            output: None,
            emit: Emit::Exe,
            options: codegen::Options::default(),
            linker: std::env::var_os("DODO_CC").unwrap_or_else(|| "cc".into()),
            link_args: vec![],
            run_args: vec![],
        },
        input: PathBuf::from("."),
        list: false,
        filters: vec![],
        skips: vec![],
        exact: false,
        ignored: false,
        include_ignored: false,
        show_output: false,
        fail_fast: false,
        timeout: Duration::from_secs(30),
        doc: false,
        no_doc: false,
        allow_empty: false,
    };
    let mut input = None;
    let mut positional = false;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        if !positional {
            match arg.to_str() {
                Some("-h" | "--help") => return Ok(Parsed::TestHelp),
                Some("--") => {
                    positional = true;
                    continue;
                }
                Some("--list") => result.list = true,
                Some("--filter") => result
                    .filters
                    .push(super::string(value("--filter")?, "filter")?),
                Some("--skip") => result.skips.push(super::string(value("--skip")?, "skip")?),
                Some("--exact") => result.exact = true,
                Some("--ignored") => result.ignored = true,
                Some("--include-ignored") => result.include_ignored = true,
                Some("--show-output") => result.show_output = true,
                Some("--fail-fast") => result.fail_fast = true,
                Some("--doc") => result.doc = true,
                Some("--no-doc") => result.no_doc = true,
                Some("--allow-empty") => result.allow_empty = true,
                Some("--timeout") => {
                    let text = super::string(value("--timeout")?, "timeout")?;
                    let seconds: f64 = text
                        .parse()
                        .map_err(|_| "--timeout requires a nonnegative number of seconds")?;
                    result.timeout = Duration::try_from_secs_f64(seconds).map_err(
                        |_| "--timeout requires a finite, nonnegative number of seconds",
                    )?;
                    if seconds > 0.0 && result.timeout.is_zero() {
                        return Err(
                            "--timeout is too small; use at least 0.000000001 seconds".into()
                        );
                    }
                }
                Some("-O" | "--opt-level") => {
                    result.compiler.options.optimization = super::optimization(&super::string(
                        value("--opt-level")?,
                        "optimization level",
                    )?)?
                }
                Some(s) if s.starts_with("-O") && s.len() > 2 => {
                    result.compiler.options.optimization = super::optimization(&s[2..])?
                }
                Some("--cpu") => {
                    result.compiler.options.cpu = Some(super::string(value("--cpu")?, "CPU")?)
                }
                Some("--features") => {
                    result.compiler.options.features =
                        super::string(value("--features")?, "features")?
                }
                Some("--linker") => result.compiler.linker = value("--linker")?,
                Some("--link-arg") => {
                    let value = value("--link-arg")?;
                    if value.to_string_lossy().starts_with("-o") {
                        return Err("test manages its temporary output path; --link-arg cannot select an output".into());
                    }
                    result.compiler.link_args.push(value);
                }
                Some(s) if s.starts_with('-') => {
                    return Err(format!("unknown test option '{s}'; use dodo test --help"));
                }
                _ => {
                    if input.replace(PathBuf::from(arg)).is_some() {
                        return Err(
                            "test accepts one file or directory; use --filter to select names"
                                .into(),
                        );
                    }
                }
            }
        } else if input.replace(PathBuf::from(arg)).is_some() {
            return Err("test accepts one file or directory".into());
        }
    }
    if result.doc && result.no_doc {
        return Err("--doc and --no-doc cannot be combined".into());
    }
    if result.ignored && result.include_ignored {
        return Err("--ignored and --include-ignored cannot be combined".into());
    }
    if result.exact && result.filters.is_empty() {
        return Err("--exact requires --filter NAME".into());
    }
    if let Some(input) = input {
        result.input = input;
    }
    Ok(Parsed::Test(Box::new(result)))
}

struct Case {
    name: String,
    id: String,
    location: String,
    ignore: Option<String>,
}

struct Unit {
    path: PathBuf,
    /// Virtual Dodo source plus its original Markdown for diagnostics.
    document: Option<(String, String)>,
    cases: Vec<Case>,
}

fn display_path(path: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    // Discovery canonicalizes paths. Match that representation on Windows,
    // where canonical paths carry a verbatim prefix and the cwd does not.
    let cwd = fs::canonicalize(&cwd).unwrap_or(cwd);
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(directory).map_err(|e| format!("cannot scan {}: {e}", directory.display()))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if kind.is_symlink() || name.to_string_lossy().starts_with('.') {
            continue;
        }
        if kind.is_dir() {
            if !super::excluded_source_directory(&name) {
                collect(&entry.path(), files)?;
            }
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| matches!(ext.to_str(), Some("dodo" | "md" | "mdx")))
        {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn candidate(source: &str) -> bool {
    match lexer::lex(source) {
        Ok(tokens) => tokens
            .windows(2)
            .any(|pair| match (&pair[0].kind, &pair[1].kind) {
                (TokenKind::Symbol("@"), TokenKind::Ident(name)) => {
                    name == "test" || name == "ignore"
                }
                (TokenKind::Ident(keyword), TokenKind::Ident(name)) => {
                    keyword == "fn" && name.starts_with("test_")
                }
                _ => false,
            }),
        // Malformed tests must not silently disappear because lexing failed.
        Err(_) => {
            source.contains("@test") || source.contains("test_") || source.contains("@ignore")
        }
    }
}

fn companion(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        name.ends_with("_test.dodo") || name.ends_with(".test.dodo")
    })
}

fn cases(path: &Path, source: &str, document_line: Option<usize>) -> Result<Vec<Case>, String> {
    let program = parser::parse(source).map_err(|d| d.render(&display_path(path), source))?;
    let mut cases = vec![];
    for f in &program.functions {
        if !(f.test || f.name.starts_with("test_") || document_line.is_some() && f.name == "main") {
            continue;
        }
        validate(f, document_line.is_some()).map_err(|message| {
            dodoc::diagnostic::Diagnostic::new(f.span, message).render(&display_path(path), source)
        })?;
        let before = &source[..f.span.start];
        let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        let path = display_path(path);
        let id = if let Some(line) = document_line {
            format!("{path}:{line}::{}", f.name)
        } else {
            format!("{path}::{}", f.name)
        };
        cases.push(Case {
            name: f.name.clone(),
            id,
            location: format!("{path}:{line}:{column}"),
            ignore: f.ignore.clone(),
        });
    }
    if let Some(line) = document_line
        && cases.is_empty()
    {
        return Err(format!(
            "{}:{}: executable documentation needs fn main() or a test function",
            display_path(path),
            line
        ));
    }
    Ok(cases)
}

fn validate(f: &Function, document: bool) -> Result<(), String> {
    let ret = f.ret == Type::Void
        || document
            && f.name == "main"
            && f.ret
                == (Type::Int {
                    signed: true,
                    bits: 32,
                });
    if f.unsafe_
        || f.extern_
        || !f.generics.is_empty()
        || !f.params.is_empty()
        || f.body.is_none()
        || !ret
        || f.name.contains('.')
    {
        return Err(format!(
            "test `{}` must be a safe, non-generic, top-level fn name() -> void",
            f.name
        ));
    }
    Ok(())
}

/// Only explicitly marked fences execute. Preserve byte offsets so diagnostics
/// point into the Markdown file, even with CRLF or non-ASCII prose before it.
fn documents(path: &Path, text: &str) -> Result<Vec<Unit>, String> {
    let mut units = vec![];
    let mut fence: Option<(u8, usize, bool, usize, usize)> = None;
    let mut offset = 0;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let bytes = trimmed.as_bytes();
        let marker = bytes.first().copied().unwrap_or(0);
        let width = bytes.iter().take_while(|b| **b == marker).count();
        if let Some((open, length, execute, start, source_line)) = fence {
            if indent <= 3
                && marker == open
                && width >= length
                && trimmed[width..].trim().is_empty()
            {
                if execute {
                    let mut source: String = text[..start]
                        .bytes()
                        .map(|b| if b == b'\n' { '\n' } else { ' ' })
                        .collect();
                    source.push_str(&text[start..offset]);
                    let cases = cases(path, &source, Some(source_line))?;
                    units.push(Unit {
                        path: path.to_owned(),
                        document: Some((source, text.to_owned())),
                        cases,
                    });
                }
                fence = None;
            }
        } else if indent <= 3 && matches!(marker, b'`' | b'~') && width >= 3 {
            let info = trimmed[width..].trim();
            let words: Vec<_> = info.split_whitespace().collect();
            let execute = words.first() == Some(&"dodo") && words.get(1) == Some(&"test");
            if execute && words.len() != 2 {
                return Err(format!(
                    "{}:{}: use exactly `dodo test` for an executable fence",
                    display_path(path),
                    index + 1
                ));
            }
            fence = Some((marker, width, execute, offset + line.len(), index + 2));
        }
        offset += line.len();
    }
    if let Some((_, _, true, _, line)) = fence {
        return Err(format!(
            "{}:{line}: unclosed executable documentation fence",
            display_path(path)
        ));
    }
    Ok(units)
}

fn discover(args: &TestArgs) -> Result<(Vec<Unit>, Vec<String>), String> {
    let input = fs::canonicalize(&args.input)
        .map_err(|e| format!("cannot open {}: {e}", args.input.display()))?;
    let explicit = input.is_file();
    let mut files = vec![];
    if explicit {
        files.push(input);
    } else {
        collect(&input, &mut files)?;
    }
    files.sort();
    let mut units = vec![];
    let mut errors = vec![];
    for path in files {
        let doc = path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "mdx");
        if (doc && args.no_doc) || (!doc && args.doc) {
            continue;
        }
        if !doc && path.extension().is_none_or(|ext| ext != "dodo") {
            return Err(
                "test inputs must be a directory, .dodo source, or .md/.mdx documentation".into(),
            );
        }
        let found = (|| {
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            if doc {
                return documents(&path, &text);
            }
            if !explicit && !companion(&path) && !candidate(&text) {
                return Ok(vec![]);
            }
            let cases = cases(&path, &text, None)?;
            Ok(vec![Unit {
                path,
                document: None,
                cases,
            }])
        })();
        match found {
            Ok(found) => units.extend(found),
            Err(error) => errors.push(error),
        }
    }
    Ok((units, errors))
}

fn load(unit: &Unit) -> Result<package::Loaded, String> {
    if let Some((source, original)) = &unit.document {
        // The overlay lives beside the document so ordinary relative imports work.
        let path = unit.path.with_extension("dodo-doctest");
        let mut loaded =
            package::load_with_overrides(&path, &BTreeMap::from([(path.clone(), source.clone())]))
                .map_err(|e| {
                    e.replace(
                        &path.display().to_string(),
                        &unit.path.display().to_string(),
                    )
                })?;
        for source in &mut loaded.sources {
            if source.path == path {
                source.path = unit.path.clone();
                // Keep the virtual source length: other files' offsets follow it.
                source.text = original[..source.text.len()].to_owned();
            }
        }
        Ok(loaded)
    } else {
        let path = if companion(&unit.path) {
            unit.path.parent().unwrap()
        } else {
            &unit.path
        };
        package::load(path)
    }
}

fn matches(args: &TestArgs, case: &Case) -> bool {
    (args.filters.is_empty()
        || args.filters.iter().any(|f| {
            if args.exact {
                case.id == *f || case.name == *f
            } else {
                case.id.contains(f)
            }
        }))
        && !args.skips.iter().any(|f| case.id.contains(f))
        && (!args.ignored || case.ignore.is_some())
}

const OUTPUT_LIMIT: u64 = 64 * 1024;
fn show_output(path: &Path, label: &str) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let truncated = file.metadata().map_err(|e| e.to_string())?.len() > OUTPUT_LIMIT;
    let mut bytes = vec![];
    file.take(OUTPUT_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if !bytes.is_empty() {
        println!(
            "  --- {label} ---\n{}",
            String::from_utf8_lossy(&bytes).trim_end()
        );
        if truncated {
            println!("  [output truncated after 64 KiB]");
        }
    }
    Ok(())
}

fn run(
    exe: &Path,
    index: usize,
    timeout: Duration,
    temporary: &Path,
) -> Result<(bool, String), String> {
    let stdout = fs::File::create(temporary.join("stdout")).map_err(|e| e.to_string())?;
    let stderr = fs::File::create(temporary.join("stderr")).map_err(|e| e.to_string())?;
    let mut command = Command::new(exe);
    command
        .arg(index.to_string())
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start test: {e}"))?;
    let start = Instant::now();
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let description = if let Some(code) = status.code() {
                    format!("exit status {code}")
                } else {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        format!("terminated by signal {}", status.signal().unwrap_or(0))
                    }
                    #[cfg(not(unix))]
                    {
                        status.to_string()
                    }
                };
                break Ok((status.success(), description));
            }
            Ok(None) => {}
            Err(error) => break Err(format!("could not wait for test: {error}")),
        }
        if !timeout.is_zero() && start.elapsed() >= timeout {
            break Ok((
                false,
                format!("timed out after {:.3}s", timeout.as_secs_f64()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // Reap timed-out tests and clean up subprocesses before the next case.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
    outcome
}

pub fn execute(mut args: TestArgs) -> Result<i32, String> {
    let started = Instant::now();
    let (mut units, errors) = discover(&args)?;
    let discovered: usize = units.iter().map(|u| u.cases.len()).sum();
    for unit in &mut units {
        unit.cases.retain(|case| matches(&args, case));
    }
    units.retain(|u| !u.cases.is_empty());
    let selected: usize = units.iter().map(|u| u.cases.len()).sum();
    let filtered = discovered - selected;
    for error in &errors {
        eprintln!("{error}");
    }
    if args.list {
        for unit in &units {
            for case in &unit.cases {
                println!(
                    "{}  ({}){}",
                    case.id,
                    case.location,
                    case.ignore
                        .as_ref()
                        .map_or(String::new(), |r| format!(" [ignored: {r}]"))
                );
            }
        }
        println!(
            "\n{selected} tests listed; {filtered} filtered out; {} discovery errors",
            errors.len()
        );
    }
    if selected == 0 {
        if !errors.is_empty() {
            println!(
                "Test discovery failed: {} errors; no runnable tests selected.",
                errors.len()
            );
            return Ok(1);
        }
        println!(
            "No tests {} in {}.\nAdd @test fn example() {{ assert_eq(2 + 2, 4) }} to a .dodo file.\nUse dodo test --list or dodo test --help to inspect discovery.",
            if discovered == 0 { "found" } else { "matched" },
            args.input.display()
        );
        return Ok(i32::from(!args.allow_empty || !errors.is_empty()));
    }
    if args.list {
        return Ok(i32::from(!errors.is_empty()));
    }
    println!("Discovered {discovered} tests; {selected} selected; {filtered} filtered out\n");
    let temporary = TempDir::new(&std::env::temp_dir())?;
    let mut passed = 0;
    let mut failed = vec![];
    let mut ignored = 0;
    let mut completed = 0;
    let bits = codegen::pointer_bits(&args.compiler.options).map_err(|e| e.to_string())?;
    for (unit_index, unit) in units.iter().enumerate() {
        let mut active = vec![];
        for case in &unit.cases {
            if let Some(reason) = &case.ignore
                && !args.include_ignored
                && !args.ignored
            {
                println!("test {} ... ignored ({reason})", case.id);
                ignored += 1;
                completed += 1;
            } else {
                active.push(case);
            }
        }
        if active.is_empty() {
            continue;
        }
        println!("Compiling {}", display_path(&unit.path));
        let _ = std::io::stdout().flush();
        let exe = temporary.0.join(format!(
            "tests-{unit_index}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let built = (|| {
            let mut loaded = load(unit)?;
            // Names are resolved in the test source's own package. Source spans
            // disambiguate names when companion files load the whole directory.
            for case in &active {
                if !loaded.program.functions.iter().any(|f| {
                    f.name == case.name
                        && loaded.source(f.span).is_some_and(|s| s.path == unit.path)
                }) {
                    return Err(format!(
                        "could not resolve test {} in its source package",
                        case.id
                    ));
                }
            }
            sema::check_for_target(&mut loaded.program, bits).map_err(|d| loaded.render(&d))?;
            args.compiler.options.test_functions = active.iter().map(|c| c.name.clone()).collect();
            args.compiler.options.test_sources = loaded.sources.clone();
            compile(&args.compiler, &loaded, &exe)
        })();
        if let Err(error) = built {
            eprintln!("{error}");
            for case in active {
                println!(
                    "test {} ... FAILED (build error; {})",
                    case.id, case.location
                );
                failed.push(case.id.clone());
                completed += 1;
            }
            if args.fail_fast {
                break;
            }
            continue;
        }
        for (index, case) in active.iter().enumerate() {
            print!("test {} ... ", case.id);
            let _ = std::io::stdout().flush();
            let start = Instant::now();
            let (success, status) =
                run(&exe, index, args.timeout, &temporary.0).unwrap_or_else(|e| (false, e));
            completed += 1;
            if success {
                passed += 1;
                println!("ok ({:.2}s)", start.elapsed().as_secs_f64());
            } else {
                failed.push(case.id.clone());
                println!("FAILED ({status}; {})", case.location);
            }
            if !success || args.show_output {
                show_output(&temporary.0.join("stdout"), "stdout")?;
                show_output(&temporary.0.join("stderr"), "stderr")?;
            }
            if !success && args.fail_fast {
                break;
            }
        }
        if args.fail_fast && !failed.is_empty() {
            break;
        }
    }
    if !failed.is_empty() {
        println!("\nFailures:");
        for id in &failed {
            println!("  {id}");
        }
        let mut command = vec![
            OsString::from("dodo"),
            "test".into(),
            args.input.as_os_str().to_owned(),
            "--exact".into(),
            "--filter".into(),
            failed[0].clone().into(),
            "--include-ignored".into(),
            "-O".into(),
            args.compiler.options.optimization.to_string().into(),
        ];
        command.extend([OsString::from("--linker"), args.compiler.linker.clone()]);
        for arg in &args.compiler.link_args {
            command.extend([OsString::from("--link-arg"), arg.clone()]);
        }
        if let Some(cpu) = &args.compiler.options.cpu {
            command.extend([OsString::from("--cpu"), cpu.into()]);
        }
        if !args.compiler.options.features.is_empty() {
            command.extend([
                OsString::from("--features"),
                args.compiler.options.features.clone().into(),
            ]);
        }
        command.extend([
            OsString::from("--timeout"),
            args.timeout.as_secs_f64().to_string().into(),
        ]);
        println!(
            "Rerun the first failure:\n  {}",
            command
                .iter()
                .map(|s| quote_arg(s))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    let success = failed.is_empty() && errors.is_empty();
    println!(
        "\nTest result: {}. {passed} passed; {} failed; {ignored} ignored; {filtered} filtered out; {} not run; {} discovery errors ({:.2}s)",
        if success { "ok" } else { "FAILED" },
        failed.len(),
        selected - completed,
        errors.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(i32::from(!success))
}

fn quote_arg(arg: &std::ffi::OsStr) -> String {
    let text = arg.to_string_lossy();
    if !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:-".contains(&b))
    {
        return text.into_owned();
    }
    // POSIX shells and PowerShell both accept single-quoted literal arguments.
    #[cfg(unix)]
    {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
    #[cfg(not(unix))]
    {
        format!("'{}'", text.replace('\'', "''"))
    }
}
