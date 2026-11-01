//! Command-line driver. Linking is an explicit process invocation, never a shell command.
use cli::{Action, Invocation, Parsed};
use dodoc::codegen::{Context, FileType};
use dodoc::hardware::{ImageFormat, Platform, esp_flash, image, serial, stk500};
use dodoc::{cli, codegen, lsp, package, project, sema, toml};
use project::{Emit, Purpose, Resolved};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
mod test_runner;

struct Args {
    action: Action,
    input: PathBuf,
    output: Option<PathBuf>,
    emit: Emit,
    options: codegen::Options,
    linker: OsString,
    link_args: Vec<OsString>,
    run_args: Vec<OsString>,
    link_cwd: Option<PathBuf>,
    run_cwd: Option<PathBuf>,
    quiet: bool,
    verbose: bool,
    manifest: Option<PathBuf>,
    profile: Option<String>,
    platform: Option<Platform>,
    /// The ESP32-C6 LP core program to embed in the firmware.
    lp_core: Option<PathBuf>,
    forbid_unsafe: bool,
}
struct FormatArgs {
    input: PathBuf,
    check: bool,
    stdout: bool,
    quiet: bool,
    verbose: bool,
}
fn host_triple() -> String {
    codegen::TargetMachine::get_default_triple()
        .as_str()
        .to_string_lossy()
        .into_owned()
}
fn codegen_options(settings: &project::Settings) -> codegen::Options {
    codegen::Options {
        target: settings.triple.clone(),
        cpu: settings.cpu.clone(),
        features: settings.features.clone().unwrap_or_default(),
        optimization: settings.opt_level.unwrap_or(0),
        debug: settings.debug.unwrap_or(false),
        panic: match settings.panic.as_ref().unwrap_or(&project::Panic::Auto) {
            project::Panic::Auto => codegen::PanicStrategy::Auto,
            project::Panic::Hosted => codegen::PanicStrategy::Hosted,
            project::Panic::Trap => codegen::PanicStrategy::Trap,
            project::Panic::Hook(s) => codegen::PanicStrategy::Hook(s.clone()),
        },
        ..Default::default()
    }
}
fn from_resolved(i: &Invocation, r: &Resolved) -> Args {
    Args {
        action: i.action,
        input: r.entry.clone().unwrap(),
        output: r.output.clone(),
        emit: r.emit,
        options: codegen_options(&r.settings),
        linker: r.settings.linker.clone().unwrap(),
        link_args: r.settings.link_args.clone().unwrap(),
        run_args: r.run_args.clone(),
        link_cwd: r.link_cwd.clone(),
        run_cwd: r.run_cwd.clone(),
        quiet: i.quiet,
        verbose: i.verbose,
        manifest: r.manifest.clone(),
        profile: r.profile.clone(),
        // Resolution already validated the board and chip names.
        platform: r.settings.platform().ok().flatten(),
        lp_core: r.settings.lp_core.clone(),
        forbid_unsafe: r.forbid_unsafe,
    }
}
fn prepare(
    i: &Invocation,
    m: Option<&project::Manifest>,
    target: Option<&str>,
    cwd: &Path,
    host: &str,
) -> Result<Resolved, String> {
    let env_linker = std::env::var_os("DODO_CC");
    let purpose = match i.action {
        Action::Check => Purpose::Check,
        Action::Run => Purpose::Run,
        Action::Test => Purpose::Test,
        _ => Purpose::Build,
    };
    let mut r = project::resolve(project::Request {
        manifest: m,
        target,
        profile: i.profile.as_deref(),
        purpose,
        overrides: &i.settings,
        link_args: &i.link_args,
        clear_link_args: i.clear_link_args,
        env_linker: env_linker.as_deref(),
        cwd,
        host,
    })?;
    if i.action == Action::Test {
        if i.manifest_path.is_some()
            && let Some(path) = &i.input
        {
            r.entry = Some(project::absolute(path, cwd));
        }
        if r.entry.is_none() {
            r.entry = Some(project::absolute(
                i.input.as_deref().unwrap_or(Path::new(".")),
                cwd,
            ));
        }
        if let Some(timeout) = i.test.timeout {
            r.timeout = timeout;
        }
    } else if r.entry.is_none() {
        let input = i.input.as_deref().unwrap_or(Path::new("."));
        r.entry = Some(if input == Path::new(".") {
            PathBuf::from("main.dodo")
        } else if input.is_dir() {
            input.join("main.dodo")
        } else {
            input.into()
        });
    }
    if let Some(emit) = i.emit {
        r.emit = emit;
        if let (Some(m), Some(name)) = (m, &r.target) {
            r.output = Some(
                m.out_dir
                    .join(r.profile.as_deref().unwrap_or("dev"))
                    .join(r.settings.triple.as_ref().unwrap())
                    .join(format!("{name}{}", project::extension(emit, &r.settings))),
            );
        }
    }
    if let Some(output) = &i.output {
        r.output = Some(project::absolute(output, cwd));
    }
    if let Some(args) = &i.run_args {
        r.run_args = args.clone();
    }
    if i.action != Action::Build {
        r.output = None;
    } else if r.output.is_none() {
        let input = r.entry.as_ref().unwrap();
        let mut name = if input.file_name().is_some_and(|n| n == "main.dodo") {
            let parent = input
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            project::absolute(parent, cwd)
                .canonicalize()
                .ok()
                .and_then(|p| p.file_name().map(OsStr::to_os_string))
        } else {
            input.file_stem().map(OsStr::to_os_string)
        }
        .unwrap_or_else(|| "program".into());
        let firmware = r.settings.board.is_some() || r.settings.chip.is_some();
        name.push(if r.emit == Emit::Exe && !firmware {
            ""
        } else {
            project::extension(r.emit, &r.settings)
        });
        r.output = Some(cwd.join("build").join(name));
    }
    let firmware = r.settings.board.is_some() || r.settings.chip.is_some();
    if i.action == Action::Run
        && (r.emit != Emit::Exe || !firmware && r.settings.triple.as_deref() != Some(host))
    {
        return Err(format!(
            "run requires a host executable; use dodo build{} for this target",
            r.target
                .as_ref()
                .map(|n| format!(" -b {n}"))
                .unwrap_or_default()
        ));
    }
    if i.action == Action::Build
        && r.emit != Emit::Exe
        && !r.settings.link_args.as_ref().unwrap().is_empty()
    {
        return Err("link-args applies only to executables; clear inherited arguments with --clear-link-args or link-args = []".into());
    }
    if !(i.action == Action::Test && (i.test.list || i.print_config)) {
        codegen::pointer_bits(&codegen_options(&r.settings)).map_err(|e| {
            let triple = r.settings.triple.as_deref().unwrap();
            if m.is_some_and(|m| m.targets.contains_key(triple)) {
                format!(
                    "{e}\n  '{triple}' is a project target; use -b {triple}, not --target {triple}"
                )
            } else {
                e.to_string()
            }
        })?;
    }
    if i.action == Action::Run
        && let Some(dir) = &r.run_cwd
        && !dir.is_dir()
    {
        return Err(format!("run.cwd is not a directory: {}", dir.display()));
    }
    Ok(r)
}
fn inspect(i: &Invocation, r: &Resolved, cwd: &Path) -> String {
    let mut text = r.report();
    if let Some(input) = &r.entry {
        let original = format!("input = {}", toml::quote(&input.to_string_lossy()));
        text = text.replace(
            &original,
            &format!(
                "input = {}",
                toml::quote(&project::absolute(input, cwd).to_string_lossy())
            ),
        );
    }
    if i.action == Action::Check {
        text.insert_str(0,"# check uses input/triple/cpu/features; emission, linking, debug, optimization and runtime fields below are unused.\n");
    }
    if i.action == Action::Test {
        text.push_str(&format!(
            concat!(
                "\n[test]\ntimeout = {}\nfilter = {}\nskip = {}\nexact = {}\n",
                "ignored = {}\ninclude-ignored = {}\ndoc = {}\nno-doc = {}\n",
                "allow-empty = {}\nshow-output = {}\nfail-fast = {}\n"
            ),
            r.timeout.as_secs_f64(),
            project::argv(
                &i.test
                    .filters
                    .iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>()
            ),
            project::argv(&i.test.skips.iter().map(OsString::from).collect::<Vec<_>>()),
            i.test.exact,
            i.test.ignored,
            i.test.include_ignored,
            i.test.doc,
            i.test.no_doc,
            i.test.allow_empty,
            i.test.show_output,
            i.test.fail_fast,
        ));
    }
    text
}
fn dispatch(i: Invocation) -> Result<i32, String> {
    match i.action {
        Action::Lsp => {
            return lsp::run(&mut std::io::stdin().lock(), &mut std::io::stdout().lock())
                .map_err(|e| format!("LSP transport failed: {e}"));
        }
        Action::Fmt => {
            return execute_format(FormatArgs {
                input: i.input.unwrap_or_else(|| ".".into()),
                check: i.fmt_check,
                stdout: i.fmt_stdout,
                quiet: i.quiet,
                verbose: i.verbose,
            });
        }
        Action::Init => return init(&i),
        Action::Completions => {
            let shell = i
                .input
                .as_ref()
                .and_then(|p| p.to_str())
                .ok_or("completions requires a shell: bash, zsh, fish, powershell")?;
            print!("{}", cli::completions(shell)?);
            return Ok(0);
        }
        _ => (),
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let host = host_triple();
    let manifest = project::discover(
        i.input.as_deref(),
        i.manifest_path.as_deref(),
        i.no_manifest,
        &cwd,
    )?;
    if i.action == Action::Targets {
        let m = manifest
            .as_ref()
            .ok_or("no dodo.toml found; select a project directory or --manifest-path PATH")?;
        if i.verbose {
            eprintln!("Manifest: {}", m.path.display());
        }
        for (name, t) in &m.targets {
            let default = m
                .default_target
                .as_ref()
                .map_or(m.targets.len() == 1, |d| d == name);
            println!(
                "{name}{}\t{}\t{}\t{}",
                if default { " (default)" } else { "" },
                t.entry.strip_prefix(&m.root).unwrap_or(&t.entry).display(),
                t.emit.name(),
                t.settings
                    .triple
                    .as_ref()
                    .or(m.build.triple.as_ref())
                    .unwrap_or(&host)
            );
        }
        return Ok(0);
    }
    if (i.build_target.is_some() || i.all_targets) && i.input.as_ref().is_some_and(|p| !p.is_dir())
    {
        return Err(
            "an explicit source file cannot be combined with project target selectors".into(),
        );
    }
    if i.all_targets && manifest.is_none() {
        return Err(
            "--all-targets requires dodo.toml; use --manifest-path PATH to select a manifest"
                .into(),
        );
    }
    let names = if i.all_targets {
        manifest
            .as_ref()
            .unwrap()
            .targets
            .keys()
            .map(|s| Some(s.as_str()))
            .collect::<Vec<_>>()
    } else {
        vec![i.build_target.as_deref()]
    };
    let mut resolved = vec![];
    for name in names {
        resolved.push(prepare(&i, manifest.as_ref(), name, &cwd, &host)?);
    }
    if i.print_config {
        for r in &resolved {
            if i.all_targets {
                println!("[[resolved]]");
            }
            print!("{}", inspect(&i, r, &cwd));
        }
        return Ok(0);
    }
    // Validate all selected entries and output destinations before starting any build.
    if i.action != Action::Test {
        for r in &resolved {
            let input = r.entry.as_ref().unwrap();
            if !input.is_file() {
                return Err(if let Some(name) = &r.target {
                    format!(
                        "targets.{name}.entry: expected source file {}",
                        input.display()
                    )
                } else if input.file_name().is_some_and(|n| n == "main.dodo") {
                    format!(
                        "expected project entry file {}; create main.dodo in this folder or pass an explicit source file",
                        input.display()
                    )
                } else {
                    format!("cannot open {}: no such source file", input.display())
                });
            }
            if let Some(output) = &r.output {
                protect_output(output, input, r.manifest.as_deref())?;
            }
        }
    }
    for notice in &i.notices {
        eprintln!("note: {notice}");
    }
    for r in resolved {
        if i.verbose {
            eprintln!("{}", inspect(&i, &r, &cwd));
        }
        let args = from_resolved(&i, &r);
        let status = if i.action == Action::Test {
            test_runner::execute(test_runner::TestArgs::new(
                args,
                r.entry.unwrap(),
                i.test.clone(),
                r.timeout,
            ))?
        } else {
            execute(args)?
        };
        if status != 0 {
            return Ok(status);
        }
    }
    Ok(0)
}
fn protect_output(output: &Path, input: &Path, manifest: Option<&Path>) -> Result<(), String> {
    if let Ok(path) = fs::canonicalize(output) {
        if fs::canonicalize(input).is_ok_and(|p| p == path) {
            return Err("output path would overwrite the source input".into());
        }
        if manifest.is_some_and(|m| fs::canonicalize(m).is_ok_and(|p| p == path)) {
            return Err("output path would overwrite the project manifest".into());
        }
        if output.extension().is_some_and(|e| e == "dodo") {
            return Err("output path must not replace a Dodo source file".into());
        }
    }
    Ok(())
}
fn init(i: &Invocation) -> Result<i32, String> {
    use std::io::Write;
    let root = i.input.as_deref().unwrap_or(Path::new("."));
    let manifest = root.join("dodo.toml");
    let main = root.join("main.dodo");
    if manifest.symlink_metadata().is_ok() {
        return Err(format!(
            "{} already exists; no files were changed",
            manifest.display()
        ));
    }
    if i.manifest_only {
        if !main.is_file() {
            return Err("--manifest-only requires an existing main.dodo".into());
        }
    } else if main.symlink_metadata().is_ok() {
        return Err("main.dodo already exists; use dodo init --manifest-only".into());
    }
    fs::create_dir_all(root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
    if !i.manifest_only {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&main)
            .and_then(|mut f| f.write_all(b"package app\n\nfn main() {}\n"))
            .map_err(|e| format!("cannot create {}: {e}", main.display()))?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest)
        .and_then(|mut f| {
            f.write_all(b"schema = 1\n\n[targets.app]\nentry = \"main.dodo\"\n\n[profiles.dev]\ndebug = true\n")
        })
        .map_err(|e| format!("cannot create {}: {e}", manifest.display()))?;
    if !i.quiet {
        eprintln!(
            "Created {}; run dodo run {}",
            manifest.display(),
            root.display()
        );
    }
    Ok(0)
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct TempDir(PathBuf);
impl TempDir {
    fn new(parent: &Path) -> Result<Self, String> {
        for _ in 0..100 {
            let parent = if parent.is_absolute() {
                parent.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(parent)
            };
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
    options.sources = loaded.sources.clone();
    let generated = codegen::generate(&context, &loaded.program, &options).map_err(|e| {
        let text = e.to_string();
        // Map an embedded "Span { start: N" to file:line for diagnosis.
        let location = text
            .split("Span { start: ")
            .nth(1)
            .and_then(|rest| rest.split(',').next())
            .and_then(|n| n.trim().parse::<usize>().ok())
            .and_then(|at| {
                loaded
                    .sources
                    .iter()
                    .filter(|s| s.start <= at && at <= s.start + s.text.len())
                    .max_by_key(|s| s.start)
                    .map(|s| {
                        let line = s.text[..at - s.start].matches('\n').count() + 1;
                        format!(" ({}:{line})", s.path.display())
                    })
            })
            .unwrap_or_default();
        format!("code generation failed: {text}{location}")
    })?;
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
            if let Some(platform) = args.platform {
                return link_firmware(args, platform, loaded, &temporary.0, &object, out);
            }
            let mut linker = Command::new(&args.linker);
            if let Some(cwd) = &args.link_cwd {
                linker.current_dir(cwd);
            }
            let target = args.options.target.clone().unwrap_or_else(|| {
                codegen::TargetMachine::get_default_triple()
                    .as_str()
                    .to_string_lossy()
                    .into_owned()
            });
            let apple = target.contains("-apple-");
            if apple {
                // The macOS SDK serves every architecture; select the target's
                // so the C runtime and the link match the emitted object.
                linker.args(["-arch", apple_arch(&target)]);
            }
            linker.arg(&object);
            if options.debug {
                linker.arg("-g");
            }
            if !args.options.test_functions.is_empty() {
                let runtime = temporary.0.join("test_runtime.c");
                fs::write(&runtime, include_str!("test_runtime.c")).map_err(|e| e.to_string())?;
                linker.arg(runtime).arg("-std=c11");
            }
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
                if target.contains("-linux-") {
                    linker.arg("-pthread");
                }
                if target.contains("-windows-") {
                    linker.args(["-lkernel32", "-lshell32"]);
                    // The signal wake channel is a loopback socket pair.
                    if native.iter().any(|(name, _)| {
                        name.starts_with("std/net/")
                            || name.starts_with("std/quic/")
                            || name.starts_with("std/signal/")
                    }) {
                        linker.arg("-lws2_32");
                    }
                }
                // Windows draw through system libraries only: X11 is spoken
                // over a socket and needs nothing beyond libc.
                if native
                    .iter()
                    .any(|(name, _)| name.starts_with("std/window/"))
                {
                    if apple {
                        linker.args([
                            "-framework",
                            "AppKit",
                            "-framework",
                            "QuartzCore",
                            "-framework",
                            "CoreGraphics",
                            "-lobjc",
                        ]);
                    }
                    if target.contains("-windows-") {
                        linker.args(["-luser32", "-lgdi32"]);
                    }
                }
                if native
                    .iter()
                    .any(|(name, _)| *name == "std/crypto/runtime.c")
                {
                    if apple {
                        linker.args(["-framework", "Security"]);
                    }
                    if target.contains("-windows-") {
                        linker.arg("-lbcrypt");
                    }
                }
                let ssl = native
                    .iter()
                    .any(|(name, _)| name.starts_with("std/tls/") || name.starts_with("std/quic/"));
                let libcrypto = native
                    .iter()
                    .any(|(name, _)| name.starts_with("std/crypto/openssl/"));
                if ssl || libcrypto {
                    if apple {
                        homebrew_openssl(&mut linker);
                    }
                    if ssl {
                        linker.arg("-lssl");
                    }
                    linker.arg("-lcrypto");
                }
            }
            // Pure-Dodo bindings have no C runtime to key on, so linking
            // follows the loaded packages.
            if apple && package::uses_metal(loaded) {
                linker.args([
                    "-framework",
                    "Metal",
                    "-framework",
                    "QuartzCore",
                    "-framework",
                    "Foundation",
                    "-lobjc",
                ]);
            }
            // std/dylib needs no C boundary; glibc before 2.34 keeps dlopen
            // in libdl, which later releases retain as an empty stub.
            if target.contains("-linux-")
                && loaded
                    .sources
                    .iter()
                    .any(|source| source.path == Path::new("<stdlib>/std/dylib/posix.dodo"))
            {
                linker.arg("-ldl");
            }
            // glibc keeps the math functions (fmod, pow, ...) that libraries
            // such as std/script/lua declare in libm.
            if target.contains("-linux-") {
                linker.arg("-lm");
            }
            linker.args(&args.link_args).arg("-o").arg(out);
            if args.verbose {
                eprintln!("Linking: {linker:?}");
            }
            let output = linker.output().map_err(|e| {
                format!(
                    "could not execute linker '{}': {e}; install a C toolchain or select --linker",
                    args.linker.to_string_lossy()
                )
            })?;
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
/// The `-arch` name Apple's toolchain uses for a target triple.
fn apple_arch(target: &str) -> &str {
    match target.split('-').next().unwrap_or_default() {
        "aarch64" => "arm64",
        arch => arch,
    }
}
/// macOS does not ship OpenSSL headers or libraries. Unless the caller chose
/// search paths, use Homebrew's OpenSSL 3 from its standard prefixes.
fn homebrew_openssl(linker: &mut Command) {
    let chosen = ["CPATH", "C_INCLUDE_PATH", "LIBRARY_PATH"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    if chosen {
        return;
    }
    for prefix in ["/opt/homebrew/opt/openssl@3", "/usr/local/opt/openssl@3"] {
        let prefix = Path::new(prefix);
        if prefix.join("include/openssl/ssl.h").is_file() {
            linker
                .arg(format!("-I{}", prefix.join("include").display()))
                .arg(format!("-L{}", prefix.join("lib").display()));
            return;
        }
    }
}
/// The flashable image written next to a firmware ELF.
fn image_path(elf: &Path, platform: Platform) -> PathBuf {
    elf.with_extension(platform.image_extension())
}
/// Link firmware from the program object and the chip's runtime, then write
/// the flashable image next to the ELF.
fn link_firmware(
    args: &Args,
    platform: Platform,
    loaded: &package::Loaded,
    temporary: &Path,
    object: &Path,
    out: &Path,
) -> Result<(), String> {
    let name = platform.name();
    if let Some((source, _)) = package::native_sources(loaded).first() {
        return Err(format!(
            "{source} needs a hosted C runtime, which '{name}' firmware does not have"
        ));
    }
    let runtime = temporary.join("runtime.o");
    codegen::compile_ir_object(
        &args.options,
        platform.chip.name,
        &(platform.chip.runtime)(),
        &runtime,
    )
    .map_err(|e| format!("cannot build the {} runtime: {e}", platform.chip.name))?;
    let script = temporary.join("memory.ld");
    fs::write(&script, platform.linker_script()).map_err(|e| e.to_string())?;
    let lp_image = match &args.lp_core {
        Some(entry) => Some(embed_lp_core(args, platform, entry, temporary)?),
        None => None,
    };
    let mut linker = Command::new(&args.linker);
    if let Some(cwd) = &args.link_cwd {
        linker.current_dir(cwd);
    }
    linker.arg(object).arg(&runtime);
    if let Some(image) = &lp_image {
        linker.arg(image);
    }
    linker
        .arg("-T")
        .arg(&script)
        .arg("--gc-sections")
        .args(&args.link_args)
        .arg("-o")
        .arg(out);
    if args.verbose {
        eprintln!("Linking: {linker:?}");
    }
    let output = linker.output().map_err(|e| {
        format!(
            "could not execute linker '{}': {e}; '{name}' firmware needs an ELF linker such as ld.lld (LLVM's lld package), or select one with --linker",
            args.linker.to_string_lossy(),
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "linker failed ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    let mut elf = fs::read(out).map_err(|e| format!("cannot read {}: {e}", out.display()))?;
    let flashable = platform.finish(&mut elf)?;
    fs::write(out, &elf).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    let path = image_path(out, platform);
    fs::write(&path, flashable).map_err(|e| format!("cannot write {}: {e}", path.display()))
}
/// Build the ESP32-C6 LP core program at `entry` for `chip = "esp32c6_lp"`,
/// and wrap its image in an object whose `.lp_image` section the main
/// firmware's linker script places in flash, between `__lp_image_start` and
/// `__lp_image_end`, for `esp32c6.LpCore` to load. The LP program uses the
/// firmware's settings, except at -O0, which its 12 KiB rarely hold: then
/// the LP chip's default level.
fn embed_lp_core(
    args: &Args,
    platform: Platform,
    entry: &Path,
    temporary: &Path,
) -> Result<PathBuf, String> {
    let chip = &dodoc::hardware::esp32c6_lp::CHIP;
    if platform.chip.name != "esp32c6" {
        return Err(format!(
            "lp-core embeds an ESP32-C6 LP core program, but this firmware is for '{}'",
            platform.name()
        ));
    }
    if !entry.is_file() {
        return Err(format!("lp-core: expected source file {}", entry.display()));
    }
    let lp_platform = Platform { chip, board: None };
    let mut options = args.options.clone();
    options.target = Some(chip.triple.into());
    options.cpu = Some(chip.cpu.into());
    options.features = chip.features.into();
    options.panic = codegen::PanicStrategy::Hook(chip.panic_hook.into());
    if options.optimization == 0 {
        options.optimization = chip.opt_level;
    }
    let lp_args = Args {
        action: Action::Build,
        input: entry.to_path_buf(),
        output: None,
        emit: Emit::Exe,
        options,
        linker: args.linker.clone(),
        link_args: vec![],
        run_args: vec![],
        link_cwd: args.link_cwd.clone(),
        run_cwd: None,
        quiet: args.quiet,
        verbose: args.verbose,
        manifest: args.manifest.clone(),
        profile: args.profile.clone(),
        platform: Some(lp_platform),
        lp_core: None,
        forbid_unsafe: args.forbid_unsafe,
    };
    let fail = |e: String| format!("LP core program {}: {e}", entry.display());
    let mut loaded =
        package::load_for_platform(entry, chip.triple, Some(lp_platform)).map_err(fail)?;
    if lp_args.forbid_unsafe {
        forbid_unsafe(&loaded).map_err(fail)?;
    }
    let bits = codegen::pointer_bits(&lp_args.options).map_err(|e| fail(e.to_string()))?;
    sema::check_for_target(&mut loaded.program, bits).map_err(|d| fail(loaded.render(&d)))?;
    let directory = temporary.join("lp_core");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let elf = directory.join("lp_core.elf");
    compile(&lp_args, &loaded, &elf).map_err(fail)?;
    let image = fs::read(image_path(&elf, lp_platform)).map_err(|e| e.to_string())?;
    let mut bytes = String::with_capacity(image.len() * 3);
    for byte in &image {
        bytes.push_str(&format!("\\{byte:02X}"));
    }
    let ir = format!(
        "@dodo_lp_image = constant [{} x i8] c\"{bytes}\", section \".lp_image\", align 4\n",
        image.len()
    );
    let object = directory.join("lp_image.o");
    codegen::compile_ir_object(&args.options, platform.chip.name, &ir, &object)
        .map_err(|e| format!("cannot embed the LP core program: {e}"))?;
    Ok(object)
}
/// Build firmware and flash it: copy it to the platform's mounted
/// bootloader drive, or write it through the board's serial bootloader or
/// the chip's serial flasher.
fn flash(args: &mut Args, platform: Platform, loaded: &package::Loaded) -> Result<i32, String> {
    if let ImageFormat::Coprocessor { host } = platform.chip.image {
        return Err(format!(
            "an {} program runs inside {host} firmware: name it with lp-core in the firmware's target (or pass --lp-core) and run that",
            platform.chip.name
        ));
    }
    if let ImageFormat::Stk500 {
        signature,
        page_size,
        baud,
    } = platform.chip.image
    {
        return flash_stk500(args, platform, loaded, signature, page_size, baud);
    }
    if let ImageFormat::EspFlash {
        chip_id,
        usb,
        download_flag,
    } = platform.chip.image
    {
        return flash_serial(args, platform, loaded, chip_id, usb, download_flag);
    }
    let mut drives = platform.bootloader_drives();
    if drives.is_empty() {
        drives = restart_into_bootloader(args, platform)?;
    }
    let drive = match drives.as_slice() {
        [drive] => drive,
        [] => {
            return Err(format!(
                "no {} bootloader drive is mounted; hold BOOTSEL while connecting the {} over USB, then run again (firmware that opens its USB serial port with the default identity, `b.usb.serial()`, lets `dodo run` restart it without BOOTSEL)",
                platform.bootloader_drive_name(),
                platform.description()
            ));
        }
        _ => {
            return Err(format!(
                "several {} bootloader drives are mounted ({}); connect one board at a time",
                platform.bootloader_drive_name(),
                drives
                    .iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    };
    let temp = TempDir::new(&std::env::temp_dir())?;
    let elf = temp.0.join("firmware.elf");
    args.emit = Emit::Exe;
    compile(args, loaded, &elf)?;
    let flashable = fs::read(image_path(&elf, platform)).map_err(|e| e.to_string())?;
    if args.verbose {
        eprintln!(
            "Flashing: {} -> {}",
            image_path(&elf, platform).display(),
            drive.display()
        );
    }
    image::write_uf2_drive(&flashable, drive)?;
    if !args.quiet {
        eprintln!(
            "Flashed {} ({} KiB) to {}",
            args.input.display(),
            flashable.len() / 2 / 1024,
            drive.display()
        );
    }
    Ok(0)
}
/// Build firmware and write it through the boot ROM's serial flasher of an
/// Espressif chip, then restart the chip into it.
fn flash_serial(
    args: &mut Args,
    platform: Platform,
    loaded: &package::Loaded,
    chip_id: u16,
    usb: serial::UsbId,
    download_flag: esp_flash::DownloadFlag,
) -> Result<i32, String> {
    let port = esp_flash::find_port(usb, platform.description())?;
    let temp = TempDir::new(&std::env::temp_dir())?;
    let elf = temp.0.join("firmware.elf");
    args.emit = Emit::Exe;
    compile(args, loaded, &elf)?;
    let flashable = fs::read(image_path(&elf, platform)).map_err(|e| e.to_string())?;
    if args.verbose {
        eprintln!(
            "Flashing: {} -> {}",
            image_path(&elf, platform).display(),
            port.path.display()
        );
    }
    let written = esp_flash::flash(
        &port,
        &flashable,
        chip_id,
        platform.flash().length,
        download_flag,
        args.verbose,
    )?;
    if !args.quiet {
        let verb = if written.unchanged {
            "Flash already holds"
        } else {
            "Flashed"
        };
        eprintln!(
            "{verb} {} ({} KiB) on {}",
            args.input.display(),
            written.bytes.div_ceil(1024),
            port.path.display()
        );
    }
    Ok(0)
}

/// Build firmware and write it through the board's STK500 serial
/// bootloader, which then starts it.
fn flash_stk500(
    args: &mut Args,
    platform: Platform,
    loaded: &package::Loaded,
    signature: [u8; 3],
    page_size: u16,
    baud: u32,
) -> Result<i32, String> {
    let port = stk500::find_port(platform.description())?;
    let temp = TempDir::new(&std::env::temp_dir())?;
    let elf = temp.0.join("firmware.elf");
    args.emit = Emit::Exe;
    compile(args, loaded, &elf)?;
    let bytes = fs::read(&elf).map_err(|e| e.to_string())?;
    let flash = platform.flash();
    let flashable = image::flat(&bytes, &image::load_segments(&bytes)?, flash.origin);
    if args.verbose {
        eprintln!("Flashing: {} -> {}", elf.display(), port.display());
    }
    let written = stk500::flash(
        &port,
        baud,
        &flashable,
        signature,
        page_size,
        flash.length,
        args.verbose,
    )?;
    if !args.quiet {
        eprintln!(
            "Flashed {} ({} of {} bytes of flash) on {}",
            args.input.display(),
            written.bytes,
            flash.length,
            port.display()
        );
    }
    Ok(0)
}
fn forbid_unsafe(loaded: &package::Loaded) -> Result<(), String> {
    let uses = package::unsafe_uses(loaded);
    if uses.is_empty() {
        return Ok(());
    }
    // Report every location at once so a project can be cleaned up in one pass.
    Err(uses.iter().map(|d| loaded.render(d)).collect())
}
/// Restart a board running firmware with a USB serial port into its
/// bootloader, by setting the port to 1200 baud, and wait for its drive.
/// Returns the mounted drives: none if no such board is attached.
fn restart_into_bootloader(args: &Args, platform: Platform) -> Result<Vec<PathBuf>, String> {
    let Some(id) = platform.chip.usb_serial else {
        return Ok(vec![]);
    };
    let ports = serial::usb_serial_ports(id);
    let port = match ports.as_slice() {
        [] => return Ok(vec![]),
        [port] => port,
        _ => {
            return Err(format!(
                "several boards with USB serial ports are connected ({}); connect one board at a time, or hold BOOTSEL on the one to flash",
                ports
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    };
    if !args.quiet {
        eprintln!(
            "Restarting the board on {} into its bootloader",
            port.display()
        );
    }
    serial::request_bootloader(port)?;
    let drives = serial::wait_for_drives(
        serial::BOOTLOADER_DRIVE_WAIT,
        std::time::Duration::from_millis(200),
        || platform.bootloader_drives(),
    );
    if !drives.is_empty() {
        return Ok(drives);
    }
    Err(format!(
        "the board on {} did not appear as the {} drive within {} seconds; its firmware may have turned off the restart (set_bootloader_reset), or hold BOOTSEL while connecting it, then run again",
        port.display(),
        platform.bootloader_drive_name(),
        serial::BOOTLOADER_DRIVE_WAIT.as_secs()
    ))
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
        codegen::TargetMachine::get_default_triple()
            .as_str()
            .to_string_lossy()
            .into_owned()
    });
    let mut loaded = package::load_for_platform(&args.input, &target, args.platform)?;
    if args.forbid_unsafe {
        forbid_unsafe(&loaded)?;
    }
    let bits = codegen::pointer_bits(&args.options).map_err(|e| e.to_string())?;
    sema::check_for_target(&mut loaded.program, bits).map_err(|d| loaded.render(&d))?;
    if args.action == Action::Check {
        if !args.quiet {
            eprintln!("Checked {}", args.input.display());
        }
        return Ok(0);
    }
    if args.action == Action::Run
        && let Some(platform) = args.platform
    {
        return flash(&mut args, platform, &loaded);
    }
    if args.action == Action::Run {
        if let Some(target) = &args.options.target
            && *target
                != codegen::TargetMachine::get_default_triple()
                    .as_str()
                    .to_string_lossy()
        {
            return Err("run requires the host target; use compile for cross compilation".into());
        }
        let temp = TempDir::new(&std::env::temp_dir())?;
        let exe = temp.0.join("program");
        args.emit = Emit::Exe;
        compile(&args, &loaded, &exe)?;
        let mut program = Command::new(&exe);
        program.args(&args.run_args);
        if let Some(cwd) = &args.run_cwd {
            program.current_dir(cwd);
        }
        if args.verbose {
            eprintln!("Running: {program:?}");
        }
        let status = program
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
            Emit::Exe if args.platform.is_some() => ".elf",
            Emit::Exe => "",
            Emit::Obj => ".o",
            Emit::Asm => ".s",
            Emit::Ir => ".ll",
            Emit::Bitcode => ".bc",
        });
        PathBuf::from("build").join(name)
    });
    protect_output(&output, &args.input, args.manifest.as_deref())?;
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
    let flashable = args
        .platform
        .filter(|_| args.emit == Emit::Exe)
        .map(|platform| (image_path(&staged, platform), image_path(&output, platform)));
    if let Some((staged, path)) = &flashable {
        fs::rename(staged, path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    if !args.quiet {
        match &flashable {
            Some((_, path)) => eprintln!("Built {} and {}", output.display(), path.display()),
            None => eprintln!("Built {}", output.display()),
        }
    }
    Ok(0)
}

// Shared by formatting and test discovery; explicit input paths bypass this filter.
fn excluded_source_directory(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.')
        || matches!(
            name.as_ref(),
            "build" | "target" | "dist" | "node_modules" | "vendor"
        )
}

fn format_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|e| format!("cannot read directory {}: {e}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", directory.display()))?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_dir() {
            if !excluded_source_directory(&entry.file_name()) {
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
    if args.verbose {
        eprintln!("Formatting {}", args.input.display());
    }
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
        if args.quiet {
            continue;
        }
        writeln!(stdout, "Formatted {}", path.display())
            .map_err(|e| format!("cannot write stdout: {e}"))?;
    }
    Ok(0)
}

fn main() {
    let code = match cli::parse(std::env::args_os().skip(1)) {
        Ok(Parsed::Help(action)) => {
            print!("{}", cli::help(action));
            0
        }
        Ok(Parsed::Version) => {
            println!("dodo {} (LLVM 23, BSD-2-Clause)", env!("CARGO_PKG_VERSION"));
            0
        }
        Ok(Parsed::Invoke(invocation)) => match dispatch(*invocation) {
            Ok(code) => code,
            Err(error) => {
                eprintln!(
                    "{}{}",
                    if error.starts_with("error:") {
                        ""
                    } else {
                        "error: "
                    },
                    error
                );
                1
            }
        },
        Err(error) => {
            eprintln!("error: {error}");
            2
        }
    };
    std::process::exit(code);
}
