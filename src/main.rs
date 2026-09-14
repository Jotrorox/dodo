//! Command-line driver. Linking is an explicit process invocation, never a shell command.
use dodoc::{codegen, lsp, package, sema};
use inkwell::context::Context;
use inkwell::targets::FileType;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};

const HELP: &str = concat!(
    "Dodo ",
    env!("CARGO_PKG_VERSION"),
    r#" — ahead-of-time systems language compiler

Usage: dodo <COMMAND> [FILE|DIRECTORY] [OPTIONS]
       dodo lsp

Commands:
  check    Parse and check types, ownership, and borrowing
  compile  Compile a native executable or compiler artifact (alias: build)
  run      Compile and run a program; arguments follow --
  lsp      Run the language server over standard input/output (alias: --lsp)
  fmt      Format and migrate syntax (default input: current directory)

Projects:
  check, compile, build, and run default to main.dodo in the current folder.
  A directory input selects its main.dodo. Explicit source files are supported.
  Import local subfolders to share code. No manifest or package manager is needed.

Formatting options:
      --check            Report unformatted files without writing
      --stdout           Print one formatted file without writing
  Use - as the fmt input to read stdin and write stdout.

Compiler options:
  -o, --output PATH       Output path (default: build/<project folder name>)
      --emit KIND         exe (default), obj, asm, llvm-ir, bitcode
  -O, --opt-level LEVEL   Optimization level: 0, 1, 2, 3 (default: 0)
      --target TRIPLE     LLVM target triple (default: host)
      --cpu NAME          Target CPU (default: generic)
      --features LIST     LLVM target features, e.g. +sse4.2
      --linker PATH       C linker driver (default: DODO_CC or cc)
      --link-arg ARG      Pass an argument to the linker; repeatable
  -h, --help             Print help
  -V, --version          Print compiler version

Examples:
  dodo run
  dodo check
  dodo compile -O 2
  dodo run -- example-argument
  dodo fmt
  dodo fmt --check
  dodo run examples/samples.dodo -O 2
  dodo compile examples/hello.dodo -o build/hello
  dodo compile examples/gpio.dodo --emit llvm-ir -o build/gpio.ll

LLVM 22 is embedded; no LLVM installation is needed to use this compiler.
Linking executables requires a C toolchain (cc, --linker, or DODO_CC).
"#
);
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Check,
    Build,
    Run,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Emit {
    Exe,
    Obj,
    Asm,
    Ir,
    Bitcode,
}
struct Args {
    action: Action,
    input: PathBuf,
    output: Option<PathBuf>,
    emit: Emit,
    options: codegen::Options,
    linker: OsString,
    link_args: Vec<OsString>,
    run_args: Vec<OsString>,
}
enum Parsed {
    Help,
    Version,
    Lsp,
    Args(Box<Args>),
    Format(FormatArgs),
}

struct FormatArgs {
    input: PathBuf,
    check: bool,
    stdout: bool,
}

fn parse_format(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut input = None;
    let mut check = false;
    let mut stdout = false;
    let mut positional = false;
    for arg in args {
        match arg.to_str() {
            Some("--help" | "-h") if !positional => return Ok(Parsed::Help),
            Some("--check") if !positional => check = true,
            Some("--stdout") if !positional => stdout = true,
            Some("--") if !positional => positional = true,
            Some(s) if !positional && s.starts_with('-') && s != "-" => {
                return Err(format!("unknown fmt option '{s}'; use dodo --help"));
            }
            _ => {
                if input.replace(PathBuf::from(arg)).is_some() {
                    return Err("fmt accepts one file or directory; omit the path to format the current directory".into());
                }
            }
        }
    }
    if check && stdout {
        return Err("fmt --check and --stdout cannot be combined".into());
    }
    Ok(Parsed::Format(FormatArgs {
        input: input.unwrap_or_else(|| PathBuf::from(".")),
        check,
        stdout,
    }))
}
fn string(value: OsString, name: &str) -> Result<String, String> {
    value
        .into_string()
        .map_err(|_| format!("{name} must be valid UTF-8"))
}
fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Parsed::Help);
    };
    let action = match command.to_str() {
        Some("-h" | "--help" | "help") => return Ok(Parsed::Help),
        Some("-V" | "--version") => return Ok(Parsed::Version),
        Some("--lsp" | "lsp") => {
            return match args.next().as_deref() {
                None => Ok(Parsed::Lsp),
                Some(arg) if arg == "--help" || arg == "-h" => Ok(Parsed::Help),
                Some(_) => Err("LSP mode takes no source path or compiler options".into()),
            };
        }
        Some("fmt") => return parse_format(args),
        Some("check") => Action::Check,
        Some("compile" | "build") => Action::Build,
        Some("run") => Action::Run,
        _ => {
            return Err(format!(
                "unknown command '{}'; use dodo --help",
                command.to_string_lossy()
            ));
        }
    };
    let mut result = Args {
        action,
        input: PathBuf::new(),
        output: None,
        emit: Emit::Exe,
        options: codegen::Options::default(),
        linker: std::env::var_os("DODO_CC").unwrap_or_else(|| "cc".into()),
        link_args: vec![],
        run_args: vec![],
    };
    let mut emit_given = false;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match arg.to_str() {
            Some("--help" | "-h") => return Ok(Parsed::Help),
            Some("--version" | "-V") => return Ok(Parsed::Version),
            Some("--") => {
                if action == Action::Run {
                    result.run_args.extend(args);
                    break;
                }
                for path in args {
                    if !result.input.as_os_str().is_empty() {
                        return Err(
                            "only one input path is accepted; pass a source file or project folder"
                                .into(),
                        );
                    }
                    result.input = path.into();
                }
                break;
            }
            Some("-o" | "--output") => {
                if result.output.is_some() {
                    return Err("output specified more than once".into());
                }
                result.output = Some(value("--output")?.into());
            }
            Some("--emit") => {
                emit_given = true;
                result.emit = match string(value("--emit")?, "emission kind")?.as_str() {
                    "exe" => Emit::Exe,
                    "obj" | "object" => Emit::Obj,
                    "asm" | "assembly" => Emit::Asm,
                    "llvm-ir" | "ir" => Emit::Ir,
                    "bitcode" | "bc" => Emit::Bitcode,
                    s => {
                        return Err(format!(
                            "unknown emission kind '{s}'; expected exe, obj, asm, llvm-ir, or bitcode"
                        ));
                    }
                };
            }
            Some("-O" | "--opt-level") => {
                result.options.optimization =
                    optimization(&string(value("--opt-level")?, "optimization level")?)?;
            }
            Some(s) if s.starts_with("-O") && s.len() > 2 => {
                result.options.optimization = optimization(&s[2..])?;
            }
            Some("--target") => result.options.target = Some(string(value("--target")?, "target")?),
            Some("--cpu") => result.options.cpu = Some(string(value("--cpu")?, "CPU")?),
            Some("--features") => {
                result.options.features = string(value("--features")?, "features")?
            }
            Some("--linker") => result.linker = value("--linker")?,
            Some("--link-arg") => {
                let v = value("--link-arg")?;
                if v == "-o" || v.to_string_lossy().starts_with("-o") {
                    return Err("use --output to choose the output path".into());
                }
                result.link_args.push(v);
            }
            Some(s) if s.starts_with('-') => {
                return Err(format!("unknown option '{s}'; use dodo --help"));
            }
            _ => {
                if !result.input.as_os_str().is_empty() {
                    return Err(
                        "only one input path is accepted; pass a source file or project folder"
                            .into(),
                    );
                }
                result.input = arg.into();
            }
        }
    }
    if result.input.as_os_str().is_empty() {
        result.input = PathBuf::from(".");
    }
    if action == Action::Check
        && (result.output.is_some() || emit_given || !result.link_args.is_empty())
    {
        return Err("check does not produce output or invoke a linker".into());
    }
    if action == Action::Run && (result.output.is_some() || emit_given) {
        return Err(
            "run builds a temporary executable; use compile to select an output artifact".into(),
        );
    }
    if result.emit != Emit::Exe && !result.link_args.is_empty() {
        return Err("--link-arg applies only to executables".into());
    }
    Ok(Parsed::Args(Box::new(result)))
}
fn optimization(s: &str) -> Result<u8, String> {
    match s {
        "0" => Ok(0),
        "1" => Ok(1),
        "2" => Ok(2),
        "3" => Ok(3),
        _ => Err(format!(
            "invalid optimization level '{s}'; expected 0, 1, 2, or 3"
        )),
    }
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct TempDir(PathBuf);
impl TempDir {
    fn new(parent: &Path) -> Result<Self, String> {
        for _ in 0..100 {
            let path = parent.join(format!(
                ".dodo-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(format!(
                        "cannot create temporary directory in {}: {e}",
                        parent.display()
                    ));
                }
            }
        }
        Err("could not allocate a temporary directory".into())
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn compile(args: &Args, loaded: &package::Loaded, out: &Path) -> Result<(), String> {
    let context = Context::create();
    let mut options = args.options.clone();
    options.entry = args.emit == Emit::Exe;
    let generated = codegen::generate(&context, &loaded.program, &options)
        .map_err(|e| format!("code generation failed: {e}"))?;
    match args.emit {
        Emit::Ir => generated
            .module
            .print_to_file(out)
            .map_err(|e| e.to_string())?,
        Emit::Bitcode => {
            if !generated.module.write_bitcode_to_path(out) {
                return Err(format!("cannot write {}", out.display()));
            }
        }
        Emit::Obj | Emit::Asm => generated
            .machine
            .write_to_file(
                &generated.module,
                if args.emit == Emit::Obj {
                    FileType::Object
                } else {
                    FileType::Assembly
                },
                out,
            )
            .map_err(|e| e.to_string())?,
        Emit::Exe => {
            let temporary = TempDir::new(out.parent().unwrap_or_else(|| Path::new(".")))?;
            let object = temporary.0.join("program.o");
            generated
                .machine
                .write_to_file(&generated.module, FileType::Object, &object)
                .map_err(|e| e.to_string())?;
            let mut linker = Command::new(&args.linker);
            linker.arg(&object);
            let native = package::native_sources(loaded);
            for (name, source) in &native {
                let file = temporary.0.join(name);
                fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
                fs::write(&file, source).map_err(|e| e.to_string())?;
                if file.extension().is_some_and(|ext| ext == "c") {
                    linker.arg(file);
                }
            }
            if !native.is_empty() {
                linker
                    .arg("-std=c11")
                    .arg(format!("-O{}", args.options.optimization));
                let target = args.options.target.clone().unwrap_or_else(|| {
                    inkwell::targets::TargetMachine::get_default_triple()
                        .as_str()
                        .to_string_lossy()
                        .into_owned()
                });
                if target.contains("-linux-") {
                    linker.arg("-pthread");
                }
                if target.contains("-windows-") {
                    linker.args(["-lkernel32", "-lshell32"]);
                    if native.iter().any(|(name, _)| name.starts_with("std/net/")) {
                        linker.arg("-lws2_32");
                    }
                }
                if native.iter().any(|(name, _)| name.starts_with("std/tls/")) {
                    linker.args(["-lssl", "-lcrypto"]);
                }
            }
            let output = linker.args(&args.link_args).arg("-o").arg(out).output().map_err(|e|format!("could not execute linker '{}': {e}; install a C toolchain or select --linker",args.linker.to_string_lossy()))?;
            if !output.status.success() {
                return Err(format!(
                    "linker failed ({}):\n{}{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }
    Ok(())
}
fn execute(mut args: Args) -> Result<i32, String> {
    // A project folder selects its entry file. Directory imports are still
    // loaded as ordinary packages by the source loader.
    if args.input == Path::new(".") {
        args.input = PathBuf::from("main.dodo");
    } else if args.input.is_dir() {
        args.input = args.input.join("main.dodo");
    }
    if args
        .input
        .file_name()
        .is_some_and(|name| name == "main.dodo")
        && !args.input.is_file()
    {
        return Err(format!(
            "expected project entry file {}; create main.dodo in this folder or pass an explicit source file",
            args.input.display()
        ));
    }
    let target = args.options.target.clone().unwrap_or_else(|| {
        inkwell::targets::TargetMachine::get_default_triple()
            .as_str()
            .to_string_lossy()
            .into_owned()
    });
    let mut loaded = package::load_for_target(&args.input, &target)?;
    let bits = codegen::pointer_bits(&args.options).map_err(|e| e.to_string())?;
    sema::check_for_target(&mut loaded.program, bits).map_err(|d| loaded.render(&d))?;
    if args.action == Action::Check {
        println!("Checked {}", args.input.display());
        return Ok(0);
    }
    if args.action == Action::Run {
        if let Some(target) = &args.options.target
            && *target
                != inkwell::targets::TargetMachine::get_default_triple()
                    .as_str()
                    .to_string_lossy()
        {
            return Err("run requires the host target; use compile for cross compilation".into());
        }
        let temp = TempDir::new(&std::env::temp_dir())?;
        let exe = temp.0.join("program");
        args.emit = Emit::Exe;
        compile(&args, &loaded, &exe)?;
        let status = Command::new(&exe)
            .args(&args.run_args)
            .status()
            .map_err(|e| format!("could not run program: {e}"))?;
        if let Some(code) = status.code() {
            return Ok(code);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let signal = status.signal().unwrap_or(1);
            eprintln!("dodo: program terminated by signal {signal}");
            return Ok(128 + signal);
        }
        #[cfg(not(unix))]
        {
            return Ok(1);
        }
    }
    let output = args.output.clone().unwrap_or_else(|| {
        let mut name = if args
            .input
            .file_name()
            .is_some_and(|name| name == "main.dodo")
        {
            args.input
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .canonicalize()
                .ok()
                .and_then(|directory| directory.file_name().map(|name| name.to_os_string()))
        } else {
            args.input.file_stem().map(|name| name.to_os_string())
        }
        .unwrap_or_else(|| OsString::from("program"));
        name.push(match args.emit {
            Emit::Exe => "",
            Emit::Obj => ".o",
            Emit::Asm => ".s",
            Emit::Ir => ".ll",
            Emit::Bitcode => ".bc",
        });
        PathBuf::from("build").join(name)
    });
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    if output.exists() {
        let path = fs::canonicalize(&output).map_err(|e| e.to_string())?;
        if path == fs::canonicalize(&args.input).map_err(|e| e.to_string())? {
            return Err("output path would overwrite the source input".into());
        }
        if output.extension().is_some_and(|e| e == "dodo") {
            return Err("output path must not replace a Dodo source file".into());
        }
    }
    let temporary = TempDir::new(parent)?;
    let staged = temporary.0.join("artifact");
    compile(&args, &loaded, &staged)?;
    fs::rename(&staged, &output).map_err(|e| format!("cannot write {}: {e}", output.display()))?;
    println!("Built {}", output.display());
    Ok(0)
}

fn format_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|e| format!("cannot read directory {}: {e}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", directory.display()))?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_dir() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with('.') && name != "target" && name != "build" {
                format_files(&path, files)?;
            }
        } else if kind.is_file() && path.extension().is_some_and(|e| e == "dodo") {
            files.push(path);
        }
    }
    Ok(())
}

fn format_source(path: &Path, source: &str) -> Result<String, String> {
    dodoc::format::format_source(source).map_err(|d| d.render(&path.to_string_lossy(), source))
}

fn execute_format(args: FormatArgs) -> Result<i32, String> {
    if args.input == Path::new("-") {
        use std::io::{Read, Write};
        let mut source = String::new();
        std::io::stdin()
            .read_to_string(&mut source)
            .map_err(|e| format!("cannot read stdin: {e}"))?;
        let formatted = format_source(Path::new("<stdin>"), &source)?;
        if args.check {
            if source != formatted {
                eprintln!("Would format <stdin>");
                return Ok(1);
            }
        } else {
            std::io::stdout()
                .write_all(formatted.as_bytes())
                .map_err(|e| format!("cannot write stdout: {e}"))?;
        }
        return Ok(0);
    }
    let input = fs::canonicalize(&args.input)
        .map_err(|e| format!("cannot open {}: {e}", args.input.display()))?;
    let mut paths = vec![];
    if input.is_dir() {
        if args.stdout {
            return Err("fmt --stdout requires a single file or stdin (-)".into());
        }
        format_files(&input, &mut paths)?;
        paths.sort();
    } else {
        paths.push(input);
    }
    // Validate every source before writing any file, including package directories.
    let mut changes = vec![];
    for path in paths {
        let source = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let formatted = format_source(&path, &source)?;
        if args.stdout {
            use std::io::Write;
            std::io::stdout()
                .write_all(formatted.as_bytes())
                .map_err(|e| format!("cannot write stdout: {e}"))?;
            return Ok(0);
        }
        if source != formatted {
            changes.push((path, source, formatted));
        }
    }
    if args.check {
        for (path, _, _) in &changes {
            eprintln!("Would format {}", path.display());
        }
        return Ok(i32::from(!changes.is_empty()));
    }
    // Stage replacements next to their targets for atomic per-file renames.
    let mut staged = vec![];
    for (path, original, formatted) in changes {
        let temporary = TempDir::new(path.parent().unwrap_or_else(|| Path::new(".")))?;
        let replacement = temporary.0.join("formatted.dodo");
        fs::write(&replacement, formatted)
            .map_err(|e| format!("cannot stage {}: {e}", path.display()))?;
        let permissions = fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions();
        fs::set_permissions(&replacement, permissions).map_err(|e| e.to_string())?;
        staged.push((path, original, replacement, temporary));
    }
    let mut formatted_paths = vec![];
    for (path, original, replacement, _temporary) in staged {
        let current = fs::read_to_string(&path)
            .map_err(|e| format!("cannot re-read {}: {e}", path.display()))?;
        if current != original {
            return Err(format!(
                "{} changed during formatting; leaving its new contents untouched",
                path.display()
            ));
        }
        fs::rename(replacement, &path)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        formatted_paths.push(path);
    }
    // A closed output pipe must not interrupt the source replacement loop.
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    for path in formatted_paths {
        writeln!(stdout, "Formatted {}", path.display())
            .map_err(|e| format!("cannot write stdout: {e}"))?;
    }
    Ok(0)
}

fn main() -> ExitCode {
    let result = match parse(std::env::args_os().skip(1)) {
        Ok(Parsed::Help) => {
            print!("{HELP}");
            Ok(0)
        }
        Ok(Parsed::Version) => {
            println!("dodo {} (LLVM 22, BSD-2-Clause)", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Ok(Parsed::Args(args)) => execute(*args),
        Ok(Parsed::Lsp) => lsp::run(&mut std::io::stdin().lock(), &mut std::io::stdout().lock())
            .map_err(|error| format!("LSP transport failed: {error}")),
        Ok(Parsed::Format(args)) => execute_format(args),
        Err(e) => Err(e),
    };
    match result {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(e) => {
            eprintln!(
                "{}{}",
                if e.starts_with("error:") {
                    ""
                } else {
                    "error: "
                },
                e
            );
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(xs: &[&str]) -> Result<Parsed, String> {
        parse(xs.iter().map(OsString::from))
    }
    #[test]
    fn invalid_options() {
        for xs in [
            &["build", "x.dodo", "-O9"][..],
            &["check", "x.dodo", "-o", "x"],
            &["run", "x.dodo", "--emit", "obj"],
            &["build", "x.dodo", "--target"],
            &["build", "x.dodo", "--wat"],
        ] {
            assert!(args(xs).is_err());
        }
    }
    #[test]
    fn run_arguments_are_preserved() {
        let Parsed::Args(a) = args(&["run", "x.dodo", "-O2", "--", "--flag", "a b"]).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            a.run_args,
            vec![OsString::from("--flag"), OsString::from("a b")]
        );
        assert_eq!(a.options.optimization, 2);
    }
    #[test]
    fn help_and_version() {
        assert!(matches!(args(&[]), Ok(Parsed::Help)));
        assert!(matches!(args(&["--version"]), Ok(Parsed::Version)));
    }

    #[test]
    fn lsp_does_not_require_a_source_or_accept_build_options() {
        for command in ["--lsp", "lsp"] {
            assert!(matches!(args(&[command]), Ok(Parsed::Lsp)));
            assert!(matches!(args(&[command, "--help"]), Ok(Parsed::Help)));
            assert!(args(&[command, "input.dodo"]).is_err());
            assert!(args(&[command, "-O2"]).is_err());
        }
    }
}
