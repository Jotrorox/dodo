//! Shared dependency-free command definitions, help, and explicit CLI overrides.
use crate::project::{self, Emit, Panic, Settings};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Build,
    Run,
    Check,
    Test,
    Fmt,
    Lsp,
    Targets,
    Init,
    Completions,
}
impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Run => "run",
            Self::Check => "check",
            Self::Test => "test",
            Self::Fmt => "fmt",
            Self::Lsp => "lsp",
            Self::Targets => "targets",
            Self::Init => "init",
            Self::Completions => "completions",
        }
    }
    fn mask(self) -> u16 {
        1 << self as u16
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "build" | "compile" => Self::Build,
            "run" => Self::Run,
            "check" => Self::Check,
            "test" => Self::Test,
            "fmt" => Self::Fmt,
            "lsp" | "--lsp" => Self::Lsp,
            "targets" => Self::Targets,
            "init" => Self::Init,
            "completions" => Self::Completions,
            _ => return None,
        })
    }
}
const B: u16 = 1;
const R: u16 = 2;
const C: u16 = 4;
const T: u16 = 8;
const F: u16 = 16;
const L: u16 = 32;
const TARGETS: u16 = 64;
const I: u16 = 128;
const COMPLETIONS: u16 = 256;
const WORK: u16 = B | R | C | T | F | TARGETS | I;
#[derive(Clone, Debug, Default)]
pub struct TestOptions {
    pub list: bool,
    pub filters: Vec<String>,
    pub skips: Vec<String>,
    pub exact: bool,
    pub ignored: bool,
    pub include_ignored: bool,
    pub show_output: bool,
    pub fail_fast: bool,
    pub timeout: Option<Duration>,
    pub doc: bool,
    pub no_doc: bool,
    pub allow_empty: bool,
}
#[derive(Clone, Debug)]
pub struct Invocation {
    pub action: Action,
    pub input: Option<PathBuf>,
    pub settings: Settings,
    pub output: Option<PathBuf>,
    pub emit: Option<Emit>,
    pub manifest_path: Option<PathBuf>,
    pub no_manifest: bool,
    pub build_target: Option<String>,
    pub all_targets: bool,
    pub profile: Option<String>,
    pub print_config: bool,
    pub clear_link_args: bool,
    pub link_args: Vec<OsString>,
    pub run_args: Option<Vec<OsString>>,
    pub quiet: bool,
    pub verbose: bool,
    pub notices: Vec<String>,
    pub fmt_check: bool,
    pub fmt_stdout: bool,
    pub test: TestOptions,
    pub manifest_only: bool,
}
impl Invocation {
    fn new(action: Action) -> Self {
        Self {
            action,
            input: None,
            settings: Settings::default(),
            output: None,
            emit: None,
            manifest_path: None,
            no_manifest: false,
            build_target: None,
            all_targets: false,
            profile: None,
            print_config: false,
            clear_link_args: false,
            link_args: vec![],
            run_args: None,
            quiet: false,
            verbose: false,
            notices: vec![],
            fmt_check: false,
            fmt_stdout: false,
            test: TestOptions::default(),
            manifest_only: false,
        }
    }
}
#[derive(Debug)]
pub enum Parsed {
    Help(Option<Action>),
    Version,
    Invoke(Box<Invocation>),
}
struct OptionSpec {
    long: &'static str,
    short: Option<char>,
    value: Option<&'static str>,
    mask: u16,
    group: &'static str,
    description: &'static str,
}
macro_rules! option {
    ($long:literal,$short:expr,$value:expr,$mask:expr,$group:literal,$description:literal) => {
        OptionSpec {
            long: $long,
            short: $short,
            value: $value,
            mask: $mask,
            group: $group,
            description: $description,
        }
    };
}
const OPTIONS: &[OptionSpec] = &[
    option!(
        "build-target",
        Some('b'),
        Some("NAME"),
        B | R | C,
        "Project",
        "Select a named target from dodo.toml"
    ),
    option!(
        "all-targets",
        None,
        None,
        B | C,
        "Project",
        "Build/check every declared target"
    ),
    option!(
        "manifest-path",
        None,
        Some("PATH"),
        B | R | C | T | TARGETS,
        "Project",
        "Use this manifest (paths normally select their own dodo.toml)"
    ),
    option!(
        "no-manifest",
        None,
        None,
        B | R | C | T,
        "Project",
        "Ignore manifests and use ordinary files/folders"
    ),
    option!(
        "release",
        None,
        None,
        B | R | T,
        "Build",
        "Use the release profile, also without a manifest"
    ),
    option!(
        "profile",
        None,
        Some("NAME"),
        B | R | T,
        "Build",
        "Profile: project default, otherwise dev"
    ),
    option!(
        "output",
        Some('o'),
        Some("PATH"),
        B,
        "Build",
        "Persistent output path (default: build/<name> without a manifest)"
    ),
    option!(
        "emit",
        None,
        Some("KIND"),
        B,
        "Build",
        "exe (default), obj, asm, llvm-ir, bitcode"
    ),
    option!(
        "debug",
        Some('g'),
        None,
        B | R | C | T,
        "Build",
        "Emit source-level DWARF debug information"
    ),
    option!(
        "no-debug",
        None,
        None,
        B | R | T,
        "Build",
        "Disable inherited debug information"
    ),
    option!(
        "opt-level",
        Some('O'),
        Some("LEVEL"),
        B | R | C | T,
        "Build",
        "Optimization: 0, 1, 2, 3 (default: 0)"
    ),
    option!(
        "target",
        None,
        Some("TRIPLE"),
        B | R | C,
        "Platform",
        "LLVM target triple (default: host); not a project target name"
    ),
    option!(
        "cpu",
        None,
        Some("NAME"),
        B | R | C | T,
        "Platform",
        "Target CPU (default: generic)"
    ),
    option!(
        "features",
        None,
        Some("LIST"),
        B | R | C | T,
        "Platform",
        "LLVM CPU features, e.g. +sse4.2; empty string clears"
    ),
    option!(
        "panic",
        None,
        Some("MODE"),
        B | R | C,
        "Runtime",
        "Runtime failure: auto (default), hosted, trap; no unwinding"
    ),
    option!(
        "panic-hook",
        None,
        Some("NAME"),
        B | R | C,
        "Runtime",
        "Non-returning C ABI board failure handler"
    ),
    option!(
        "linker",
        None,
        Some("PATH"),
        B | R | C | T,
        "Linking",
        "C linker driver (DODO_CC, otherwise cc)"
    ),
    option!(
        "link-arg",
        None,
        Some("ARG"),
        B | R | T,
        "Linking",
        "Append one linker argument; repeatable, e.g. --link-arg=-s"
    ),
    option!(
        "clear-link-args",
        None,
        None,
        B | R | T,
        "Linking",
        "Clear inherited linker args before appending CLI args"
    ),
    option!(
        "check",
        None,
        None,
        F,
        "Formatting",
        "Report unformatted files without writing (exit 1 for differences)"
    ),
    option!(
        "stdout",
        None,
        None,
        F,
        "Formatting",
        "Print one formatted file without writing"
    ),
    option!(
        "list",
        None,
        None,
        T,
        "Tests",
        "List matching tests without compiling"
    ),
    option!(
        "filter",
        None,
        Some("TEXT"),
        T,
        "Tests",
        "Match name or path; repeatable OR"
    ),
    option!(
        "skip",
        None,
        Some("TEXT"),
        T,
        "Tests",
        "Exclude matching names; repeatable"
    ),
    option!(
        "exact",
        None,
        None,
        T,
        "Tests",
        "Match filters against whole name or path::name"
    ),
    option!("ignored", None, None, T, "Tests", "Run only ignored tests"),
    option!(
        "include-ignored",
        None,
        None,
        T,
        "Tests",
        "Also run ignored tests"
    ),
    option!(
        "show-output",
        None,
        None,
        T,
        "Tests",
        "Show captured output for passing tests too"
    ),
    option!(
        "fail-fast",
        None,
        None,
        T,
        "Tests",
        "Stop after the first build/test failure"
    ),
    option!(
        "timeout",
        None,
        Some("SECONDS"),
        T,
        "Tests",
        "Per-test deadline (default: 30; 0 disables)"
    ),
    option!(
        "doc",
        None,
        None,
        T,
        "Tests",
        "Discover only executable Markdown examples"
    ),
    option!(
        "no-doc",
        None,
        None,
        T,
        "Tests",
        "Discover only Dodo source tests"
    ),
    option!(
        "allow-empty",
        None,
        None,
        T,
        "Tests",
        "Succeed when no tests match (default: exit 1)"
    ),
    option!(
        "manifest-only",
        None,
        None,
        I,
        "Project",
        "Create dodo.toml for an existing main.dodo"
    ),
    option!(
        "print-config",
        None,
        None,
        B | R | C | T,
        "Inspection",
        "Print resolved TOML configuration and exit without compiling"
    ),
    option!(
        "quiet",
        Some('q'),
        None,
        WORK,
        "Output",
        "Suppress progress/success messages, not errors or program output"
    ),
    option!(
        "verbose",
        Some('v'),
        None,
        WORK,
        "Output",
        "Show configuration and subprocess invocations"
    ),
    option!(
        "help",
        Some('h'),
        None,
        WORK | L | COMPLETIONS,
        "Information",
        "Print command help"
    ),
    option!(
        "version",
        Some('V'),
        None,
        WORK | L | COMPLETIONS,
        "Information",
        "Print compiler version"
    ),
];
const COMMANDS: &[&str] = &[
    "build",
    "compile",
    "run",
    "check",
    "test",
    "fmt",
    "lsp",
    "targets",
    "init",
    "completions",
    "help",
];
fn utf8(value: OsString, name: &str) -> Result<String, String> {
    value
        .into_string()
        .map_err(|_| format!("{name} must be valid UTF-8"))
}
fn unused_on_check(name: &str) -> bool {
    matches!(
        name,
        "debug" | "opt-level" | "panic" | "panic-hook" | "linker"
    )
}
fn unexpected(action: Action, token: &str) -> String {
    let options = OPTIONS
        .iter()
        .filter(|s| s.mask & action.mask() != 0)
        .map(|s| format!("--{}", s.long))
        .collect::<Vec<_>>();
    let mut message = format!(
        "unexpected option '{token}' for 'dodo {}'{}; use dodo {} --help",
        action.name(),
        project::suggest(token, options.iter().map(String::as_str)),
        action.name()
    );
    if action == Action::Run {
        message
            .push_str("\n  program arguments must follow '--'; example: dodo run -- --port 8080");
    }
    message
}
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut args = args.into_iter().peekable();
    let Some(command) = args.next() else {
        return Ok(Parsed::Help(None));
    };
    match command.to_str() {
        Some("-h" | "--help") => return Ok(Parsed::Help(None)),
        Some("-V" | "--version") => return Ok(Parsed::Version),
        Some("help") => {
            let action = args
                .next()
                .map(|s| {
                    Action::parse(&s.to_string_lossy()).ok_or_else(|| {
                        format!(
                            "unknown help command '{}'{}",
                            s.to_string_lossy(),
                            project::suggest(&s.to_string_lossy(), COMMANDS.iter().copied())
                        )
                    })
                })
                .transpose()?;
            if args.next().is_some() {
                return Err("help accepts one command name".into());
            }
            return Ok(Parsed::Help(action));
        }
        _ => (),
    }
    let action = Action::parse(&command.to_string_lossy()).ok_or_else(|| {
        format!(
            "unknown command '{}'{}; use dodo --help",
            command.to_string_lossy(),
            project::suggest(&command.to_string_lossy(), COMMANDS.iter().copied())
        )
    })?;
    let mut invocation = Invocation::new(action);
    let mut positional = false;
    let mut release = false;
    let mut profile_given = false;
    while let Some(token) = args.next() {
        if !positional && token == "--" {
            if action == Action::Run {
                invocation.run_args = Some(args.collect());
                break;
            }
            positional = true;
            continue;
        }
        if positional || !token.as_encoded_bytes().starts_with(b"-") || token == "-" {
            if matches!(action, Action::Lsp) {
                return Err("LSP mode takes no source path or compiler options".into());
            }
            if invocation.input.replace(PathBuf::from(token)).is_some() {
                return Err(format!("{} accepts one input path", action.name()));
            }
            continue;
        }
        let bytes = token.as_encoded_bytes();
        let (spec, attached) = if bytes.starts_with(b"--") {
            let end = bytes.iter().position(|b| *b == b'=').unwrap_or(bytes.len());
            let name = std::str::from_utf8(&bytes[2..end]).unwrap_or("");
            let spec = OPTIONS
                .iter()
                .find(|s| s.long == name && s.mask & action.mask() != 0)
                .ok_or_else(|| unexpected(action, &token.to_string_lossy()))?;
            // Splitting an OsStr at an ASCII delimiter preserves its native encoding.
            let value = (end < bytes.len()).then(|| {
                unsafe { OsStr::from_encoded_bytes_unchecked(&bytes[end + 1..]) }.to_os_string()
            });
            (spec, value)
        } else {
            let short = bytes.get(1).copied().unwrap_or_default() as char;
            let spec = OPTIONS
                .iter()
                .find(|s| s.short == Some(short) && s.mask & action.mask() != 0)
                .ok_or_else(|| unexpected(action, &token.to_string_lossy()))?;
            let value = (bytes.len() > 2).then(|| {
                unsafe { OsStr::from_encoded_bytes_unchecked(&bytes[2..]) }.to_os_string()
            });
            (spec, value)
        };
        let value = if let Some(value_name) = spec.value {
            match attached {
                Some(value) => value,
                None => {
                    let Some(next) = args.peek() else {
                        return Err(format!("--{} requires {}", spec.long, value_name));
                    };
                    if spec.long != "link-arg"
                        && !(spec.long == "features"
                            && next.as_encoded_bytes().starts_with(b"-")
                            && !next.as_encoded_bytes().starts_with(b"--"))
                        && next.as_encoded_bytes().starts_with(b"-")
                    {
                        return Err(format!(
                            "--{} requires a value; use --{}=VALUE for a value beginning with '-'",
                            spec.long, spec.long
                        ));
                    }
                    args.next().unwrap()
                }
            }
        } else {
            if attached.is_some() {
                return Err(format!("--{} does not take a value", spec.long));
            }
            OsString::new()
        };
        if action == Action::Check && unused_on_check(spec.long) {
            invocation.notices.push(format!(
                "--{} has no effect on check (accepted for compatibility)",
                spec.long
            ));
        }
        match spec.long {
            "help" => return Ok(Parsed::Help(Some(action))),
            "version" => return Ok(Parsed::Version),
            "build-target" => invocation.build_target = Some(utf8(value, "build target")?),
            "all-targets" => invocation.all_targets = true,
            "manifest-path" => invocation.manifest_path = Some(value.into()),
            "no-manifest" => invocation.no_manifest = true,
            "profile" => {
                profile_given = true;
                invocation.profile = Some(utf8(value, "profile")?);
            }
            "release" => release = true,
            "output" => {
                if invocation.output.replace(value.into()).is_some() {
                    return Err("output specified more than once".into());
                }
            }
            "emit" => invocation.emit = Some(Emit::parse(&utf8(value, "emission kind")?)?),
            "debug" => invocation.settings.debug = Some(true),
            "no-debug" => invocation.settings.debug = Some(false),
            "opt-level" => {
                let s = utf8(value, "optimization level")?;
                invocation.settings.opt_level = Some(match s.as_str() {
                    "0" => 0,
                    "1" => 1,
                    "2" => 2,
                    "3" => 3,
                    _ => {
                        return Err(format!(
                            "invalid optimization level '{s}'; expected 0, 1, 2, or 3"
                        ));
                    }
                });
            }
            "target" => invocation.settings.triple = Some(utf8(value, "target")?),
            "cpu" => invocation.settings.cpu = Some(utf8(value, "CPU")?),
            "features" => invocation.settings.features = Some(utf8(value, "features")?),
            "panic" => invocation.settings.panic = Some(Panic::parse(&utf8(value, "panic mode")?)?),
            "panic-hook" => {
                invocation.settings.panic = Some(Panic::hook(utf8(value, "panic hook")?)?)
            }
            "linker" => {
                if value.is_empty() {
                    return Err("--linker requires a nonempty executable name or path".into());
                }
                invocation.settings.linker = Some(value);
            }
            "link-arg" => invocation.link_args.push(value),
            "clear-link-args" => invocation.clear_link_args = true,
            "check" => invocation.fmt_check = true,
            "stdout" => invocation.fmt_stdout = true,
            "list" => invocation.test.list = true,
            "filter" => invocation.test.filters.push(utf8(value, "filter")?),
            "skip" => invocation.test.skips.push(utf8(value, "skip")?),
            "exact" => invocation.test.exact = true,
            "ignored" => invocation.test.ignored = true,
            "include-ignored" => invocation.test.include_ignored = true,
            "show-output" => invocation.test.show_output = true,
            "fail-fast" => invocation.test.fail_fast = true,
            "timeout" => {
                let s = utf8(value, "timeout")?;
                invocation.test.timeout = Some(project::timeout(s.parse().map_err(|_| {
                    "--timeout requires a nonnegative number of seconds".to_string()
                })?)?);
            }
            "doc" => invocation.test.doc = true,
            "no-doc" => invocation.test.no_doc = true,
            "allow-empty" => invocation.test.allow_empty = true,
            "print-config" => invocation.print_config = true,
            "quiet" => invocation.quiet = true,
            "verbose" => invocation.verbose = true,
            "manifest-only" => invocation.manifest_only = true,
            _ => unreachable!(),
        }
    }
    for (conflict, message) in [
        (
            release && profile_given,
            "--release and --profile cannot be combined",
        ),
        (
            invocation.quiet && invocation.verbose,
            "--quiet and --verbose cannot be combined",
        ),
        (
            invocation.all_targets && invocation.build_target.is_some(),
            "--all-targets and --build-target cannot be combined",
        ),
        (
            invocation.all_targets && invocation.output.is_some(),
            "--all-targets and --output cannot be combined",
        ),
        (
            invocation.no_manifest && invocation.manifest_path.is_some(),
            "--no-manifest and --manifest-path cannot be combined",
        ),
        (
            invocation.manifest_path.is_some()
                && invocation.input.is_some()
                && action != Action::Test,
            "--manifest-path cannot be combined with an input path",
        ),
        (
            invocation.no_manifest && (invocation.build_target.is_some() || invocation.all_targets),
            "target selectors cannot be combined with --no-manifest",
        ),
        (
            invocation.fmt_check && invocation.fmt_stdout,
            "fmt --check and --stdout cannot be combined",
        ),
        (
            invocation.test.doc && invocation.test.no_doc,
            "--doc and --no-doc cannot be combined",
        ),
        (
            invocation.test.ignored && invocation.test.include_ignored,
            "--ignored and --include-ignored cannot be combined",
        ),
        (
            invocation.test.exact && invocation.test.filters.is_empty(),
            "--exact requires --filter NAME",
        ),
        (
            invocation.test.list && invocation.print_config,
            "--list and --print-config cannot be combined",
        ),
    ] {
        if conflict {
            return Err(message.into());
        }
    }
    if release {
        invocation.profile = Some("release".into());
    }
    if action == Action::Completions {
        let shell = invocation
            .input
            .as_ref()
            .and_then(|s| s.to_str())
            .ok_or("completions requires a shell: bash, zsh, fish, powershell")?;
        if !matches!(shell, "bash" | "zsh" | "fish" | "powershell") {
            return Err("expected a completion shell: bash, zsh, fish, powershell".into());
        }
    }
    project::validate_link_args(&invocation.link_args)?;
    Ok(Parsed::Invoke(Box::new(invocation)))
}
pub fn help(action: Option<Action>) -> String {
    let mut out = format!(
        "Dodo {} — ahead-of-time systems language compiler\n\n",
        env!("CARGO_PKG_VERSION")
    );
    let Some(action) = action else {
        out.push_str(
            "Usage: dodo <COMMAND> [OPTIONS]

Commands:
  build        Build an executable or compiler artifact (alias: compile)
  run          Build and run; program arguments follow --
  check        Check types, ownership, and borrowing
  test         Discover and run tests
  fmt          Format source files
  targets      List targets from dodo.toml
  init         Create a project without replacing existing files
  completions  Generate shell completions
  lsp          Start the language server (alias: --lsp)
  help         Show command help

Examples:
  dodo run
  dodo build --release
  dodo test --filter addition

Projects need no manifest. Add dodo.toml for named targets and saved settings.
Use dodo <COMMAND> --help or dodo help <COMMAND> for options.
Use dodo --version to print the compiler version.
https://jotrorox.github.io/dodo/command-line/
",
        );
        return out;
    };
    let usage = match action {
        Action::Lsp => "",
        Action::Completions => "<bash|zsh|fish|powershell>",
        Action::Init | Action::Targets => "[DIRECTORY] [OPTIONS]",
        Action::Run => "[FILE|DIRECTORY] [OPTIONS] [-- PROGRAM_ARGS...]",
        _ => "[FILE|DIRECTORY] [OPTIONS]",
    };
    out.push_str(&format!("Usage: dodo {} {usage}\n", action.name()));
    let description = match action {
        Action::Build => {
            "Build a persistent executable or compiler artifact. A directory selects its dodo.toml or main.dodo. Explicit files bypass manifests. LLVM 23 is embedded; executable linking needs a C toolchain."
        }
        Action::Run => {
            "Build a temporary executable and run it on the host. Arguments after -- replace saved run.args; an empty -- clears them. The program's exit status is preserved."
        }
        Action::Check => {
            "Check an entry and its imports for the selected platform. No main function or linker is required."
        }
        Action::Test => {
            "Recursively discover @test/test_ functions and executable Markdown examples. Each test runs in an isolated host process. Hidden paths, build, target, dist, node_modules, vendor, and symlinks are skipped."
        }
        Action::Fmt => {
            "Format files recursively (default: current directory). Use - for stdin/stdout. --stdout requires a single file; --check writes nothing."
        }
        Action::Lsp => {
            "Serve the language server protocol over stdin/stdout. No source path or compiler options are accepted."
        }
        Action::Targets => {
            "List named project targets, entries, output kinds, and platform triples from dodo.toml. This does not list all LLVM platforms."
        }
        Action::Init => {
            "Create dodo.toml and main.dodo in a directory (default: current directory). Existing files are never overwritten."
        }
        Action::Completions => {
            "Write a shell completion script to stdout. Source it or install it using your shell's completion directory."
        }
    };
    out.push_str(&format!("\n{description}\n"));
    for group in [
        "Project",
        "Build",
        "Tests",
        "Formatting",
        "Platform",
        "Linking",
        "Runtime",
        "Inspection",
        "Output",
        "Information",
    ] {
        let options = OPTIONS
            .iter()
            .filter(|s| {
                s.mask & action.mask() != 0
                    && s.group == group
                    && !(action == Action::Check && unused_on_check(s.long))
            })
            .collect::<Vec<_>>();
        if options.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{group} options:\n"));
        for s in options {
            let flags = format!(
                "{}--{}{}",
                s.short.map(|c| format!("-{c}, ")).unwrap_or_default(),
                s.long,
                s.value.map(|v| format!(" {v}")).unwrap_or_default()
            );
            out.push_str(&format!("  {flags:<29} {}\n", s.description));
        }
    }
    out.push_str("\nExamples:\n");
    out.push_str(match action {
        Action::Build => "  dodo build --release\n  dodo build -b server -g\n  dodo build main.dodo --emit=llvm-ir -o build/main.ll\n",
        Action::Run => "  dodo run\n  dodo run -b server --release -- --port 8080\n",
        Action::Check => "  dodo check\n  dodo check -b wasm\n",
        Action::Test => "  dodo test --list\n  dodo test -g --filter addition\n  dodo test tests --manifest-path dodo.toml --release\n",
        Action::Fmt => "  dodo fmt\n  dodo fmt --check .\n  dodo fmt --stdout main.dodo\n",
        Action::Targets => "  dodo targets\n  dodo targets path/to/project\n",
        Action::Init => "  dodo init hello\n  dodo init --manifest-only\n",
        Action::Completions => "  dodo completions bash\n",
        Action::Lsp => "  dodo lsp\n",
    });
    out
}
/// Completion words come from the same command/option definitions as parsing.
pub fn completions(shell: &str) -> Result<String, String> {
    let words = COMMANDS
        .iter()
        .map(|s| s.to_string())
        .chain(OPTIONS.iter().map(|s| format!("--{}", s.long)))
        .collect::<Vec<_>>()
        .join(" ");
    match shell {
        "bash" => Ok(format!(
            "_dodo_complete() {{ COMPREPLY=( $(compgen -W '{words}' -- \"${{COMP_WORDS[COMP_CWORD]}}\") ); }}\ncomplete -o default -F _dodo_complete dodo\n"
        )),
        "zsh" => Ok(format!(
            "#compdef dodo\n_arguments '*:argument:({words})'\n"
        )),
        "fish" => Ok(format!("complete -c dodo -a '{words}'\n")),
        "powershell" => Ok(format!(
            "Register-ArgumentCompleter -Native -CommandName dodo -ScriptBlock {{ param($wordToComplete, $commandAst, $cursorPosition) '{words}'.Split(' ') | Where-Object {{ $_.StartsWith($wordToComplete) }} | ForEach-Object {{ [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_) }} }}\n"
        )),
        _ => Err("expected a completion shell: bash, zsh, fish, powershell".into()),
    }
}
