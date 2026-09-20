//! Optional project manifests and deterministic configuration resolution.
//! This module does not inspect process environment, change directories, or use LLVM.
use crate::toml::{self, Kind, Value};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Emit {
    #[default]
    Exe,
    Obj,
    Asm,
    Ir,
    Bitcode,
}
impl Emit {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "exe" => Ok(Self::Exe),
            "obj" | "object" => Ok(Self::Obj),
            "asm" | "assembly" => Ok(Self::Asm),
            "llvm-ir" | "ir" => Ok(Self::Ir),
            "bitcode" | "bc" => Ok(Self::Bitcode),
            _ => Err(format!(
                "unknown emission kind '{s}'; expected exe, obj, asm, llvm-ir, or bitcode"
            )),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Exe => "exe",
            Self::Obj => "obj",
            Self::Asm => "asm",
            Self::Ir => "llvm-ir",
            Self::Bitcode => "bitcode",
        }
    }
    pub fn extension(self, triple: &str) -> &'static str {
        match self {
            Self::Exe if triple.contains("-windows-") => ".exe",
            Self::Exe => "",
            Self::Obj => ".o",
            Self::Asm => ".s",
            Self::Ir => ".ll",
            Self::Bitcode => ".bc",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Panic {
    Auto,
    Hosted,
    Trap,
    Hook(String),
}
impl Panic {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "auto" => Ok(Self::Auto),
            "hosted" => Ok(Self::Hosted),
            "trap" => Ok(Self::Trap),
            _ => Err(format!(
                "invalid panic mode '{s}'; expected auto, hosted, or trap"
            )),
        }
    }
    pub fn hook(s: String) -> Result<Self, String> {
        if s.is_empty()
            || !s
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
        {
            Err("panic hook must be a C symbol name".into())
        } else {
            Ok(Self::Hook(s))
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub opt_level: Option<u8>,
    pub debug: Option<bool>,
    pub triple: Option<String>,
    pub cpu: Option<String>,
    pub features: Option<String>,
    pub panic: Option<Panic>,
    pub linker: Option<OsString>,
    pub link_args: Option<Vec<OsString>>,
}
impl Settings {
    fn overlay(&mut self, other: &Self, source: &str, origins: &mut BTreeMap<String, String>) {
        macro_rules! field {
            ($field:ident, $key:literal) => {
                if let Some(value) = &other.$field {
                    self.$field = Some(value.clone());
                    origins.insert($key.into(), source.into());
                }
            };
        }
        field!(opt_level, "opt-level");
        field!(debug, "debug");
        field!(triple, "triple");
        field!(cpu, "cpu");
        field!(features, "features");
        field!(panic, "panic");
        field!(linker, "linker");
        field!(link_args, "link-args");
    }
}
#[derive(Clone, Debug)]
pub struct Target {
    pub entry: PathBuf,
    pub emit: Emit,
    pub settings: Settings,
    pub run_args: Vec<OsString>,
    pub run_cwd: Option<PathBuf>,
}
#[derive(Clone, Debug)]
pub struct Manifest {
    pub path: PathBuf,
    pub root: PathBuf,
    pub name: Option<String>,
    pub default_target: Option<String>,
    pub default_profile: Option<String>,
    pub build: Settings,
    pub out_dir: PathBuf,
    pub targets: BTreeMap<String, Target>,
    pub profiles: BTreeMap<String, Settings>,
    pub test_root: PathBuf,
    pub test_timeout: Duration,
    pub test_build: Settings,
}
type Fields = BTreeMap<String, Value>;
type ParseResult<T> = Result<T, toml::Error>;
fn error(v: &Value, message: impl Into<String>) -> toml::Error {
    toml::Error {
        offset: v.offset,
        message: message.into(),
    }
}
fn table(v: &Value) -> ParseResult<&Fields> {
    v.as_table().ok_or_else(|| error(v, "expected a table"))
}
fn known(t: &Fields, keys: &[&str]) -> ParseResult<()> {
    for (key, value) in t {
        if !keys.contains(&key.as_str()) {
            return Err(error(
                value,
                format!(
                    "unknown field '{key}'{}",
                    suggest(key, keys.iter().copied())
                ),
            ));
        }
    }
    Ok(())
}
fn string(v: &Value) -> ParseResult<String> {
    match &v.kind {
        Kind::String(s) if !s.contains('\0') => Ok(s.clone()),
        _ => Err(error(v, "expected a string without NUL characters")),
    }
}
fn optional_string(t: &Fields, key: &str) -> ParseResult<Option<String>> {
    t.get(key).map(string).transpose()
}
fn strings(v: &Value) -> ParseResult<Vec<OsString>> {
    match &v.kind {
        Kind::Array(a) => a.iter().map(|v| string(v).map(OsString::from)).collect(),
        _ => Err(error(v, "expected an array of strings")),
    }
}
fn boolean(v: &Value) -> ParseResult<bool> {
    if let Kind::Bool(b) = v.kind {
        Ok(b)
    } else {
        Err(error(v, "expected a boolean"))
    }
}
fn level(v: &Value) -> ParseResult<u8> {
    match v.kind {
        Kind::Integer(n) if (0..=3).contains(&n) => Ok(n as u8),
        _ => Err(error(v, "opt-level must be an integer from 0 to 3")),
    }
}
pub fn timeout(seconds: f64) -> Result<Duration, String> {
    let duration = Duration::try_from_secs_f64(seconds)
        .map_err(|_| "timeout requires a finite, nonnegative number of seconds".to_string())?;
    if seconds > 0.0 && duration.is_zero() {
        return Err("timeout is too small; use at least 0.000000001 seconds".into());
    }
    Ok(duration)
}
const SETTINGS: &[&str] = &[
    "opt-level",
    "debug",
    "triple",
    "cpu",
    "features",
    "panic",
    "panic-hook",
    "linker",
    "link-args",
];
fn settings(t: &Fields, root: &Path) -> ParseResult<Settings> {
    if let (Some(_), Some(hook)) = (t.get("panic"), t.get("panic-hook")) {
        return Err(error(
            hook,
            "panic and panic-hook cannot appear in the same table",
        ));
    }
    let mut result = Settings {
        opt_level: t.get("opt-level").map(level).transpose()?,
        debug: t.get("debug").map(boolean).transpose()?,
        triple: optional_string(t, "triple")?,
        cpu: optional_string(t, "cpu")?,
        features: optional_string(t, "features")?,
        linker: optional_string(t, "linker")?.map(|s| linker_path(OsStr::new(&s), root)),
        link_args: t.get("link-args").map(strings).transpose()?,
        ..Default::default()
    };
    if let Some(v) = t.get("panic") {
        result.panic = Some(Panic::parse(&string(v)?).map_err(|e| error(v, e))?);
    }
    if let Some(v) = t.get("panic-hook") {
        result.panic = Some(Panic::hook(string(v)?).map_err(|e| error(v, e))?);
    }
    for key in ["triple", "cpu", "linker"] {
        if let Some(v) = t.get(key)
            && string(v)?.is_empty()
        {
            return Err(error(v, format!("{key} must not be empty")));
        }
    }
    if let Some(args) = &result.link_args {
        validate_link_args(args).map_err(|e| error(&t["link-args"], e))?;
    }
    Ok(result)
}
pub fn validate_link_args(args: &[OsString]) -> Result<(), String> {
    if args.iter().any(|s| s.to_string_lossy().starts_with("-o")) {
        Err(
            "use --output to choose the output path; linker arguments cannot select an output"
                .into(),
        )
    } else {
        Ok(())
    }
}
pub fn valid_name(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let reserved = matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || (lower.starts_with("com") || lower.starts_with("lpt"))
            && lower.len() == 4
            && matches!(lower.as_bytes()[3], b'1'..=b'9');
    !s.is_empty()
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        && !reserved
}
fn names(t: &Fields) -> ParseResult<()> {
    let mut seen = BTreeMap::new();
    for (name, v) in t {
        if !valid_name(name) {
            return Err(error(
                v,
                format!(
                    "invalid target/profile name '{name}'; use letters, digits, '-' or '_', starting with a letter or digit, and avoid reserved Windows names"
                ),
            ));
        }
        if let Some(previous) = seen.insert(name.to_ascii_lowercase(), name) {
            return Err(error(
                v,
                format!("'{name}' collides with '{previous}' on case-insensitive filesystems"),
            ));
        }
    }
    Ok(())
}
impl Manifest {
    pub fn read(path: &Path) -> Result<Self, String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read manifest {}: {e}", path.display()))?;
        Self::parse(&source, path)
    }
    /// `path` should be absolute; the driver owns resolution against the caller.
    pub fn parse(source: &str, path: &Path) -> Result<Self, String> {
        let parse = || -> ParseResult<Self> {
            let document = toml::parse(source)?;
            let top = table(&document)?;
            known(
                top,
                &[
                    "schema", "project", "build", "targets", "profiles", "test", "tool",
                ],
            )?;
            let schema = top
                .get("schema")
                .ok_or_else(|| error(&document, "missing required schema = 1"))?;
            if !matches!(schema.kind, Kind::Integer(1)) {
                return Err(error(
                    schema,
                    "unsupported manifest schema; expected schema = 1",
                ));
            }
            let root = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let mut result = Self {
                path: path.into(),
                root: root.clone(),
                name: None,
                default_target: None,
                default_profile: None,
                build: Settings::default(),
                out_dir: root.join("build"),
                targets: BTreeMap::new(),
                profiles: BTreeMap::new(),
                test_root: root.clone(),
                test_timeout: Duration::from_secs(30),
                test_build: Settings::default(),
            };
            if let Some(v) = top.get("project") {
                let t = table(v)?;
                known(t, &["name", "default-target", "default-profile"])?;
                result.name = optional_string(t, "name")?;
                result.default_target = optional_string(t, "default-target")?;
                result.default_profile = optional_string(t, "default-profile")?;
            }
            if let Some(v) = top.get("build") {
                let t = table(v)?;
                let mut keys = SETTINGS.to_vec();
                keys.push("out-dir");
                known(t, &keys)?;
                result.build = settings(t, &root)?;
                if let Some(p) = optional_string(t, "out-dir")? {
                    result.out_dir = root.join(p);
                }
            }
            if let Some(v) = top.get("targets") {
                let targets = table(v)?;
                names(targets)?;
                for (name, v) in targets {
                    let t = table(v)?;
                    let mut keys = SETTINGS.to_vec();
                    keys.extend(["entry", "emit", "run"]);
                    known(t, &keys)?;
                    let entry = t
                        .get("entry")
                        .ok_or_else(|| error(v, format!("targets.{name}.entry is required")))?;
                    let entry_path = PathBuf::from(string(entry)?);
                    if entry_path.extension().is_none_or(|e| e != "dodo") {
                        return Err(error(entry, "entry must name a .dodo source file"));
                    }
                    let emit = t
                        .get("emit")
                        .map(|v| {
                            let s = string(v)?;
                            let emit = Emit::parse(&s).map_err(|e| error(v, e))?;
                            if emit.name() != s {
                                return Err(error(
                                    v,
                                    "use a canonical emit value: exe, obj, asm, llvm-ir, bitcode",
                                ));
                            }
                            Ok(emit)
                        })
                        .transpose()?
                        .unwrap_or_default();
                    let mut target = Target {
                        entry: root.join(entry_path),
                        emit,
                        settings: settings(t, &root)?,
                        run_args: vec![],
                        run_cwd: None,
                    };
                    if let Some(v) = t.get("run") {
                        let run = table(v)?;
                        known(run, &["args", "cwd"])?;
                        if let Some(v) = run.get("args") {
                            target.run_args = strings(v)?;
                        }
                        target.run_cwd = optional_string(run, "cwd")?.map(|p| root.join(p));
                    }
                    result.targets.insert(name.clone(), target);
                }
            }
            if result.targets.is_empty() {
                result.targets.insert(
                    "app".into(),
                    Target {
                        entry: root.join("main.dodo"),
                        emit: Emit::Exe,
                        settings: Settings::default(),
                        run_args: vec![],
                        run_cwd: None,
                    },
                );
            }
            if let Some(v) = top.get("profiles") {
                let profiles = table(v)?;
                names(profiles)?;
                for (name, v) in profiles {
                    if matches!(name.to_ascii_lowercase().as_str(), "dev" | "release")
                        && name != &name.to_ascii_lowercase()
                    {
                        return Err(error(
                            v,
                            format!("profile '{name}' collides with a built-in profile"),
                        ));
                    }
                    let t = table(v)?;
                    known(t, &["opt-level", "debug"])?;
                    result.profiles.insert(name.clone(), settings(t, &root)?);
                }
            }
            if let Some(v) = top.get("test") {
                let t = table(v)?;
                known(t, &["root", "timeout", "build"])?;
                if let Some(p) = optional_string(t, "root")? {
                    result.test_root = root.join(p);
                }
                if let Some(v) = t.get("timeout") {
                    let seconds = match v.kind {
                        Kind::Integer(n) => n as f64,
                        Kind::Float(n) => n,
                        _ => return Err(error(v, "timeout must be a number of seconds")),
                    };
                    result.test_timeout = timeout(seconds).map_err(|e| error(v, e))?;
                }
                if let Some(v) = t.get("build") {
                    let t = table(v)?;
                    known(
                        t,
                        &[
                            "opt-level",
                            "debug",
                            "cpu",
                            "features",
                            "linker",
                            "link-args",
                        ],
                    )?;
                    result.test_build = settings(t, &root)?;
                }
            }
            if let Some(v) = top.get("tool") {
                table(v)?;
            }
            if let Some(name) = &result.default_target
                && !result.targets.contains_key(name)
            {
                return Err(error(
                    &top["project"],
                    format!("unknown default-target '{name}'"),
                ));
            }
            if let Some(name) = &result.default_profile
                && !matches!(name.as_str(), "dev" | "release")
                && !result.profiles.contains_key(name)
            {
                return Err(error(
                    &top["project"],
                    format!("unknown default-profile '{name}'"),
                ));
            }
            Ok(result)
        };
        parse().map_err(|e| e.render(source, path))
    }
    pub fn select(&self, name: Option<&str>) -> Result<(&str, &Target), String> {
        let name = name.or(self.default_target.as_deref()).or_else(|| {
            if self.targets.len() == 1 {
                self.targets.keys().next().map(String::as_str)
            } else {
                None
            }
        });
        let choices = self.targets.keys().map(String::as_str).collect::<Vec<_>>();
        let Some(name) = name else {
            return Err(format!(
                "select a build target with -b NAME or set project.default-target; available targets: {}",
                choices.join(", ")
            ));
        };
        self.targets
            .get_key_value(name)
            .map(|(name, target)| (name.as_str(), target))
            .ok_or_else(|| {
                format!(
                    "unknown build target '{name}'{}\n  available targets: {}\n  use dodo targets to inspect the project",
                    suggest(name, choices.iter().copied()),
                    choices.join(", ")
                )
            })
    }
}
pub fn absolute(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.into()
    } else {
        cwd.join(path)
    }
}
pub fn linker_path(value: &OsStr, base: &Path) -> OsString {
    if Path::new(value).components().count() > 1
        || value.as_encoded_bytes().contains(&b'/')
        || cfg!(windows) && value.as_encoded_bytes().contains(&b'\\')
    {
        absolute(Path::new(value), base).into_os_string()
    } else {
        value.into()
    }
}
/// Only absence permits fallback; dangling symlinks and unreadable files are errors.
pub fn discover(
    input: Option<&Path>,
    manifest_path: Option<&Path>,
    no_manifest: bool,
    cwd: &Path,
) -> Result<Option<Manifest>, String> {
    if no_manifest {
        return Ok(None);
    }
    let path = if let Some(path) = manifest_path {
        absolute(path, cwd)
    } else {
        let directory = absolute(input.unwrap_or(Path::new(".")), cwd);
        if !directory.is_dir() {
            return Ok(None);
        }
        let path = directory.join("dodo.toml");
        match std::fs::symlink_metadata(&path) {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot inspect {}: {e}", path.display())),
        }
        path
    };
    Manifest::read(&path).map(Some)
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Build,
    Run,
    Check,
    Test,
}
pub struct Request<'a> {
    pub manifest: Option<&'a Manifest>,
    pub target: Option<&'a str>,
    pub profile: Option<&'a str>,
    pub purpose: Purpose,
    pub overrides: &'a Settings,
    pub link_args: &'a [OsString],
    pub clear_link_args: bool,
    pub env_linker: Option<&'a OsStr>,
    pub cwd: &'a Path,
    pub host: &'a str,
}
#[derive(Debug)]
pub struct Resolved {
    pub settings: Settings,
    pub origins: BTreeMap<String, String>,
    pub target: Option<String>,
    pub profile: Option<String>,
    pub manifest: Option<PathBuf>,
    pub entry: Option<PathBuf>,
    pub emit: Emit,
    pub output: Option<PathBuf>,
    pub link_cwd: Option<PathBuf>,
    pub run_cwd: Option<PathBuf>,
    pub run_args: Vec<OsString>,
    pub timeout: Duration,
}
pub fn resolve(request: Request<'_>) -> Result<Resolved, String> {
    let Request {
        manifest,
        target,
        profile,
        purpose,
        overrides,
        link_args,
        clear_link_args,
        env_linker,
        cwd,
        host,
    } = request;
    let mut r = Resolved {
        settings: Settings::default(),
        origins: BTreeMap::new(),
        target: None,
        profile: None,
        manifest: manifest.map(|m| m.path.clone()),
        entry: None,
        emit: Emit::Exe,
        output: None,
        link_cwd: manifest.map(|m| m.root.clone()),
        run_cwd: manifest.map(|m| m.root.clone()),
        run_args: vec![],
        timeout: Duration::from_secs(30),
    };
    r.settings.overlay(
        &Settings {
            opt_level: Some(0),
            debug: Some(false),
            triple: Some(host.into()),
            cpu: Some("generic".into()),
            features: Some(String::new()),
            panic: Some(Panic::Auto),
            linker: Some("cc".into()),
            link_args: Some(vec![]),
        },
        "compiler defaults",
        &mut r.origins,
    );
    if let Some(m) = manifest {
        if purpose == Purpose::Test {
            r.settings
                .overlay(&m.test_build, "test.build", &mut r.origins);
            r.entry = Some(m.test_root.clone());
            r.timeout = m.test_timeout;
        } else {
            let (name, t) = m.select(target)?;
            r.target = Some(name.into());
            r.entry = Some(t.entry.clone());
            r.emit = t.emit;
            r.run_args = t.run_args.clone();
            r.run_cwd = Some(t.run_cwd.clone().unwrap_or_else(|| m.root.clone()));
            r.settings.overlay(&m.build, "build", &mut r.origins);
            r.settings
                .overlay(&t.settings, &format!("targets.{name}"), &mut r.origins);
        }
    } else if target.is_some() {
        return Err(
            "--build-target requires dodo.toml; use --manifest-path PATH to select a manifest"
                .into(),
        );
    }
    if purpose != Purpose::Check {
        let name = profile
            .or_else(|| manifest.and_then(|m| m.default_profile.as_deref()))
            .unwrap_or("dev");
        if name != "dev"
            && name != "release"
            && manifest.is_none_or(|m| !m.profiles.contains_key(name))
        {
            let mut choices = vec!["dev", "release"];
            if let Some(m) = manifest {
                choices.extend(m.profiles.keys().map(String::as_str));
            }
            choices.sort();
            choices.dedup();
            return Err(format!(
                "unknown profile '{name}'{}; available profiles: {}",
                suggest(name, choices.iter().copied()),
                choices.join(", ")
            ));
        }
        if name == "release" {
            r.settings.overlay(
                &Settings {
                    opt_level: Some(3),
                    debug: Some(false),
                    ..Default::default()
                },
                "built-in release profile",
                &mut r.origins,
            );
        }
        if let Some(settings) = manifest.and_then(|m| m.profiles.get(name)) {
            r.settings
                .overlay(settings, &format!("profiles.{name}"), &mut r.origins);
        }
        if manifest.is_some() || profile.is_some() {
            r.profile = Some(name.into());
        }
    }
    if let Some(linker) = env_linker {
        r.settings.linker = Some(linker_path(linker, cwd));
        r.origins.insert("linker".into(), "DODO_CC".into());
    }
    let mut explicit = overrides.clone();
    explicit.linker = explicit.linker.as_ref().map(|s| linker_path(s, cwd));
    r.settings.overlay(&explicit, "CLI", &mut r.origins);
    if clear_link_args {
        r.settings.link_args = Some(vec![]);
        r.origins.insert("link-args".into(), "CLI (cleared)".into());
    }
    if !link_args.is_empty() {
        r.settings
            .link_args
            .as_mut()
            .unwrap()
            .extend_from_slice(link_args);
        let origin = r.origins.get_mut("link-args").unwrap();
        origin.push_str(" + CLI");
    }
    let triple = r.settings.triple.as_ref().unwrap();
    if triple.is_empty()
        || !triple
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || triple == "."
        || triple == ".."
    {
        return Err("target must be an LLVM target triple without path separators".into());
    }
    if let (Some(m), Some(name)) = (manifest, &r.target)
        && purpose != Purpose::Check
    {
        r.output = Some(
            m.out_dir
                .join(r.profile.as_deref().unwrap_or("dev"))
                .join(triple)
                .join(format!("{name}{}", r.emit.extension(triple))),
        );
    }
    Ok(r)
}
impl Resolved {
    pub fn report(&self) -> String {
        let mut lines = vec![];
        let mut add = |key: &str, value: String, origin: &str| {
            lines.push(format!("{key} = {value} # {origin}"));
        };
        if let Some(path) = &self.manifest {
            add(
                "manifest",
                toml::quote(&path.to_string_lossy()),
                "selected manifest",
            );
        }
        if let Some(name) = &self.target {
            add("build-target", toml::quote(name), "target selection");
        }
        if let Some(profile) = &self.profile {
            add("profile", toml::quote(profile), "profile selection");
        }
        if let Some(path) = &self.entry {
            add(
                "input",
                toml::quote(&path.to_string_lossy()),
                "resolved input",
            );
        }
        if let Some(path) = &self.output {
            add(
                "output",
                toml::quote(&path.to_string_lossy()),
                "resolved output",
            );
        }
        add(
            "emit",
            toml::quote(self.emit.name()),
            "selected target / CLI",
        );
        let s = &self.settings;
        for (key, value) in [
            ("opt-level", s.opt_level.unwrap().to_string()),
            ("debug", s.debug.unwrap().to_string()),
            ("triple", toml::quote(s.triple.as_deref().unwrap())),
            ("cpu", toml::quote(s.cpu.as_deref().unwrap())),
            ("features", toml::quote(s.features.as_deref().unwrap())),
            (
                "linker",
                toml::quote(&s.linker.as_ref().unwrap().to_string_lossy()),
            ),
            ("link-args", argv(s.link_args.as_ref().unwrap())),
        ] {
            add(key, value, &self.origins[key]);
        }
        let (key, value) = match s.panic.as_ref().unwrap() {
            Panic::Auto => ("panic", "auto"),
            Panic::Hosted => ("panic", "hosted"),
            Panic::Trap => ("panic", "trap"),
            Panic::Hook(s) => ("panic-hook", s.as_str()),
        };
        add(key, toml::quote(value), &self.origins["panic"]);
        if let Some(path) = &self.link_cwd {
            add(
                "link-cwd",
                toml::quote(&path.to_string_lossy()),
                "manifest root",
            );
        }
        if let Some(path) = &self.run_cwd {
            add(
                "run-cwd",
                toml::quote(&path.to_string_lossy()),
                "manifest root / run.cwd",
            );
        }
        add("run-args", argv(&self.run_args), "run.args / CLI");
        lines.join("\n") + "\n"
    }
}
pub fn argv(args: &[OsString]) -> String {
    format!(
        "[{}]",
        args.iter()
            .map(|s| toml::quote(&s.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
pub fn suggest<'a>(input: &str, choices: impl IntoIterator<Item = &'a str>) -> String {
    if input.len() > 128 {
        return String::new();
    }
    let mut best = vec![];
    let mut min = 3usize;
    for choice in choices {
        let mut row: Vec<usize> = (0..=choice.chars().count()).collect();
        for (i, a) in input.chars().enumerate() {
            let mut prev = row[0];
            row[0] = i + 1;
            for (j, b) in choice.chars().enumerate() {
                let old = row[j + 1];
                row[j + 1] = (prev + usize::from(a != b)).min(row[j] + 1).min(old + 1);
                prev = old;
            }
        }
        let distance = *row.last().unwrap();
        if distance < min {
            min = distance;
            best = vec![choice];
        } else if distance == min {
            best.push(choice);
        }
    }
    if best.len() == 1 && min <= 2 {
        format!("; did you mean '{}'?", best[0])
    } else {
        String::new()
    }
}
