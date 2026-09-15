//! Package loading, namespace resolution, and source-aware diagnostics.
//!
//! A file loads that file. An imported directory loads its immediate `.dodo`
//! files in lexical order, as does an explicit directory load from an editor.
//! The CLI selects `main.dodo` before calling the loader for a project folder.
//! `core/*`, `alloc/*`, and `std/*` resolve from bundled sources. Other
//! imports resolve relative to the importing package, without network access or
//! an implicit dependency cache.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::parser;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/stdlib_sources.rs"));

const INTRINSIC_IMPORTS: &[&str] = &["core/mem", "core/ptr", "core/mmio"];

/// Distinct identities prevent a local path from impersonating a bundled source.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ModuleId {
    Local(PathBuf),
    Bundled(String),
}

impl ModuleId {
    fn path(&self) -> PathBuf {
        match self {
            Self::Local(path) => path.clone(),
            Self::Bundled(import) => PathBuf::from("<stdlib>").join(format!("{import}.dodo")),
        }
    }
}

impl std::fmt::Display for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.path().display().fmt(f)
    }
}

#[derive(Clone, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub text: String,
    pub start: usize,
}

#[derive(Clone, Debug)]
pub struct Loaded {
    pub program: Program,
    pub sources: Vec<Source>,
    /// Editor syntax diagnostics; strict loading leaves this empty.
    pub diagnostics: Vec<Diagnostic>,
    /// Per-source package prefix (empty key) and visible import aliases.
    pub(crate) namespaces: BTreeMap<PathBuf, BTreeMap<String, String>>,
}

impl Loaded {
    pub fn source(&self, span: Span) -> Option<&Source> {
        self.sources.iter().find(|source| {
            span.start >= source.start && span.start <= source.start + source.text.len()
        })
    }

    pub fn render(&self, diagnostic: &Diagnostic) -> String {
        let paths: Vec<_> = self
            .sources
            .iter()
            .map(|source| source.path.display().to_string())
            .collect();
        let sources: Vec<_> = self
            .sources
            .iter()
            .zip(&paths)
            .map(|(source, path)| (path.as_str(), source.text.as_str(), source.start))
            .collect();
        diagnostic.render_with_sources(&sources)
    }
}

/// A loading error retains the original diagnostic and source for editor clients.
#[derive(Debug)]
pub struct LoadError {
    pub diagnostic: Diagnostic,
    pub source: Option<Box<Source>>,
}

impl From<String> for LoadError {
    fn from(message: String) -> Self {
        Self {
            diagnostic: Diagnostic::new(Span::default(), message),
            source: None,
        }
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(source) = &self.source {
            f.write_str(
                &self
                    .diagnostic
                    .render(&source.path.display().to_string(), &source.text),
            )
        } else {
            self.diagnostic.fmt(f)
        }
    }
}

impl std::error::Error for LoadError {}

pub fn load(path: &Path) -> Result<Loaded, String> {
    load_with_overlays(path, &BTreeMap::new()).map_err(|error| error.to_string())
}

/// Select each hosted adapter independently using the compilation target.
pub fn load_for_target(path: &Path, target: &str) -> Result<Loaded, String> {
    load_with_overlays_for_target(path, &BTreeMap::new(), target).map_err(|error| error.to_string())
}

/// C boundaries are embedded alongside the Dodo sources for relocated compilers.
pub fn native_sources(loaded: &Loaded) -> Vec<(&'static str, &'static str)> {
    BUNDLED_NATIVE_SOURCES
        .iter()
        .copied()
        .filter(|(name, _)| {
            let directory = Path::new(name).parent().unwrap();
            loaded.sources.iter().any(|source| {
                source.path.parent() == Some(Path::new("<stdlib>").join(directory).as_path())
                    && source
                        .path
                        .file_stem()
                        .is_some_and(|stem| stem == "linux" || stem == "windows")
            })
        })
        .collect()
}

/// Load editor buffers with string errors for callers of the original overlay API.
/// Keys use the same normalized absolute paths as `load_with_overlays`.
pub fn load_with_overrides(
    path: &Path,
    overrides: &BTreeMap<PathBuf, String>,
) -> Result<Loaded, String> {
    load_with_overlays(path, overrides).map_err(|error| error.to_string())
}

/// Load a file or package, preferring unsaved text over files on disk.
/// Overlay keys must be absolute paths normalized with `source_path`.
pub fn load_with_overlays(
    path: &Path,
    overlays: &BTreeMap<PathBuf, String>,
) -> Result<Loaded, LoadError> {
    load_with_overlays_for_target(path, overlays, env!("DODO_HOST_TARGET"))
}

pub fn load_with_overlays_for_target(
    path: &Path,
    overlays: &BTreeMap<PathBuf, String>,
    target: &str,
) -> Result<Loaded, LoadError> {
    load_internal(ModuleId::Local(source_path(path)), overlays, target, false)
}

/// Recover syntax in editor buffers and dependencies without weakening builds.
pub fn load_for_editor(
    path: &Path,
    overlays: &BTreeMap<PathBuf, String>,
    target: &str,
) -> Result<Loaded, LoadError> {
    load_internal(ModuleId::Local(source_path(path)), overlays, target, true)
}

/// Look up a compiler-owned source by its internal path, never on disk.
#[cfg(feature = "llvm")]
pub(crate) fn bundled_source(path: &Path) -> Option<&'static str> {
    let name = path.strip_prefix("<stdlib>").ok()?.to_str()?;
    let name = name.replace('\\', "/");
    let name = name.strip_suffix(".dodo")?;
    BUNDLED_SOURCES
        .iter()
        .find(|(import, _)| name == *import)
        .map(|(_, text)| *text)
}

/// Check a bundled source as an editor document, preserving its library identity.
#[cfg(feature = "llvm")]
pub(crate) fn load_bundled_for_editor(path: &Path, target: &str) -> Result<Loaded, LoadError> {
    let name = path
        .strip_prefix("<stdlib>")
        .ok()
        .and_then(|path| path.to_str())
        .and_then(|name| name.strip_suffix(".dodo"))
        .ok_or_else(|| "invalid bundled source path".to_owned())?;
    load_internal(
        ModuleId::Bundled(name.replace('\\', "/")),
        &BTreeMap::new(),
        target,
        true,
    )
}

fn load_internal(
    root: ModuleId,
    overlays: &BTreeMap<PathBuf, String>,
    target: &str,
    recover: bool,
) -> Result<Loaded, LoadError> {
    let mut loader = Loader {
        overlays,
        recover,
        target: target.to_owned(),
        ..Loader::default()
    };
    let root = loader.module(root, None)?;
    let mut program = Program::default();
    // Keep familiar names when unique, but package identity is its resolved path,
    // never its final component. Every module resolves its own local aliases.
    let mut counts = BTreeMap::<String, usize>::new();
    for module in loader.modules.values() {
        *counts.entry(module.alias.clone()).or_default() += 1;
    }
    let mut known_packages: BTreeSet<String> = counts.keys().cloned().collect();
    known_packages.extend(["mem", "ptr", "mmio"].map(str::to_owned));
    let mut prefixes = BTreeMap::new();
    for (index, (id, module)) in loader.modules.iter().enumerate() {
        let prefix = if id == &root {
            String::new()
        } else if counts[&module.alias] == 1 {
            module.alias.clone()
        } else {
            let mut candidate = format!("__dodo_package_{index}");
            while known_packages.contains(&candidate) {
                candidate.push('_');
            }
            candidate
        };
        known_packages.insert(prefix.clone());
        prefixes.insert(id.clone(), prefix);
    }
    for intrinsic in INTRINSIC_IMPORTS {
        prefixes.insert(
            ModuleId::Bundled((*intrinsic).to_owned()),
            intrinsic.rsplit('/').next().unwrap().to_owned(),
        );
    }
    let mut namespaces = BTreeMap::new();
    for (key, mut module) in loader.modules {
        for function in &module.program.functions {
            if function.printing.is_some()
                && !matches!(&key, ModuleId::Bundled(path) if path == "std/fmt" || path == "std/console")
            {
                return Err(
                    "@compiler printing declarations are reserved for std/fmt and std/console"
                        .to_owned()
                        .into(),
                );
            }
        }
        if key == root {
            program.package = module.program.package.clone();
        }
        let mut visible_packages: BTreeMap<String, String> = module
            .import_aliases
            .iter()
            .map(|(alias, id)| (alias.clone(), prefixes[id].clone()))
            .collect();
        visible_packages.insert(module.alias.clone(), prefixes[&key].clone());
        namespace(
            &mut module.program,
            &prefixes[&key],
            &known_packages,
            &visible_packages,
        )
        .map_err(|error| format!("{key}: {error}"))?;
        visible_packages.insert(String::new(), prefixes[&key].clone());
        for path in module.sources {
            namespaces.insert(path, visible_packages.clone());
        }
        append(&mut program, module.program);
    }
    program.imports.sort();
    program.imports.dedup();
    Ok(Loaded {
        program,
        sources: loader.sources,
        diagnostics: loader.diagnostics,
        namespaces,
    })
}

struct Module {
    sources: Vec<PathBuf>,
    program: Program,
    alias: String,
    import_aliases: BTreeMap<String, ModuleId>,
}

struct Loader<'a> {
    recover: bool,
    diagnostics: Vec<Diagnostic>,
    target: String,
    overlays: &'a BTreeMap<PathBuf, String>,
    modules: BTreeMap<ModuleId, Module>,
    loading: Vec<ModuleId>,
    sources: Vec<Source>,
    offset: usize,
}

impl Default for Loader<'_> {
    fn default() -> Self {
        static EMPTY: BTreeMap<PathBuf, String> = BTreeMap::new();
        Self {
            recover: false,
            diagnostics: vec![],
            target: String::new(),
            overlays: &EMPTY,
            modules: BTreeMap::new(),
            loading: vec![],
            sources: vec![],
            offset: 0,
        }
    }
}

impl Loader<'_> {
    fn native_import(&self, import: &str) -> Result<String, LoadError> {
        let parts: Vec<_> = import.split('/').collect();
        if parts.len() != 3
            || parts[0] != "std"
            || !matches!(
                parts[1],
                "platform"
                    | "fs"
                    | "process"
                    | "env"
                    | "thread"
                    | "sync"
                    | "net"
                    | "tls"
                    | "web"
                    | "time"
            )
            || !matches!(parts[2], "native" | "linux" | "windows")
        {
            return Ok(import.to_owned());
        }
        let adapter = if self.target.starts_with("x86_64-")
            && self.target.contains("-linux-")
            && self.target.ends_with("-gnu")
        {
            "linux"
        } else if self.target.starts_with("x86_64-")
            && self.target.contains("-windows-")
            && (self.target.ends_with("-msvc") || self.target.ends_with("-gnu"))
        {
            "windows"
        } else {
            return Err(format!("`{import}` is unsupported for target `{}`; hosted adapters require x86_64 Linux GNU or Windows (MSVC/GNU)", self.target).into());
        };
        if parts[2] != "native" && parts[2] != adapter {
            return Err(format!("`{import}` cannot be used for target `{}`; select `std/{}/native` or the `{adapter}` adapter", self.target, parts[1]).into());
        }
        Ok(format!("std/{}/{adapter}", parts[1]))
    }

    fn module(
        &mut self,
        id: ModuleId,
        expected_alias: Option<&str>,
    ) -> Result<ModuleId, LoadError> {
        let path = id.path();
        if matches!(id, ModuleId::Local(_)) && !self.overlays.contains_key(&path) {
            path.canonicalize()
                .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        }
        if let Some(index) = self.loading.iter().position(|entry| entry == &id) {
            let mut cycle: Vec<String> = self.loading[index..]
                .iter()
                .map(ToString::to_string)
                .collect();
            cycle.push(path.display().to_string());
            return Err(format!("cyclic package import: {}", cycle.join(" -> ")).into());
        }
        if let Some(module) = self.modules.get(&id) {
            if expected_alias.is_some_and(|alias| alias != module.alias) {
                return Err(format!(
                    "{} declares package `{}`, which differs from its import name",
                    path.display(),
                    module.alias
                )
                .into());
            }
            return Ok(id);
        }
        if self.loading.len() >= 128 {
            return Err("package import nesting exceeds the supported limit of 128"
                .to_owned()
                .into());
        }
        self.loading.push(id.clone());
        let files = match &id {
            ModuleId::Local(_) => source_files(&path, self.overlays)?,
            ModuleId::Bundled(_) => vec![path.clone()],
        };
        let directory = if path.is_dir() {
            path.as_path()
        } else {
            path.parent().unwrap_or(Path::new("."))
        };
        let mut program = Program::default();
        let source_paths = files.clone();
        for file in files {
            let text = match &id {
                ModuleId::Bundled(import) => BUNDLED_SOURCES
                    .iter()
                    .find(|(name, _)| name == import)
                    .map(|(_, text)| (*text).to_owned())
                    .ok_or_else(|| format!("unknown standard library import `{import}`"))?,
                ModuleId::Local(_) => match self.overlays.get(&file) {
                    Some(text) => text.clone(),
                    None => std::fs::read_to_string(&file).map_err(|error| {
                        format!("cannot read UTF-8 source {}: {error}", file.display())
                    })?,
                },
            };
            let mut unit = if self.recover {
                let (unit, diagnostics) = parser::parse_recovering(&text);
                for mut diagnostic in diagnostics {
                    shift(&mut diagnostic.span, self.offset);
                    for label in &mut diagnostic.labels {
                        shift(&mut label.span, self.offset);
                    }
                    self.diagnostics.push(diagnostic);
                }
                unit
            } else {
                parser::parse(&text).map_err(|diagnostic| LoadError {
                    diagnostic,
                    source: Some(Box::new(Source {
                        path: file.clone(),
                        text: text.clone(),
                        start: 0,
                    })),
                })?
            };
            if program.package.is_empty() {
                program.package = unit.package.clone();
            }
            if program.package != unit.package {
                return Err(format!(
                    "{} declares package `{}`; all files in this directory must declare `{}`",
                    file.display(),
                    unit.package,
                    program.package
                )
                .into());
            }
            let mut imports = BTreeSet::new();
            for import in &unit.imports {
                if !imports.insert(import) {
                    return Err(
                        format!("{} imports `{import}` more than once", file.display()).into(),
                    );
                }
            }
            shift_program(&mut unit, self.offset);
            self.sources.push(Source {
                path: file,
                text,
                start: self.offset,
            });
            self.offset = self
                .offset
                .checked_add(self.sources.last().map_or(0, |source| source.text.len()))
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| "source input is too large".to_owned())?;
            append(&mut program, unit);
        }
        let alias = program.package.clone();
        if alias == "core" {
            return Err(format!(
                "{} declares package `core`, which is reserved for compiler intrinsics",
                path.display()
            )
            .into());
        }
        if expected_alias.is_some_and(|expected| expected != alias) {
            return Err(format!(
                "{} declares package `{alias}`; expected `{}` to match the import's final component",
                path.display(),
                expected_alias.unwrap_or_default()
            ).into());
        }
        if matches!(alias.as_str(), "mem" | "ptr" | "mmio") {
            return Err(format!(
                "conflicting package name `{alias}`: reserved for compiler intrinsics"
            )
            .into());
        }
        let mut import_aliases = BTreeMap::new();
        import_aliases.insert(alias.clone(), id.clone());
        let mut explicit_aliases = BTreeMap::new();
        for (import, name) in &program.import_aliases {
            if let Some(previous) = explicit_aliases.insert(import.clone(), name.clone())
                && previous != *name
            {
                return Err(format!(
                    "conflicting aliases for import `{import}` in {}",
                    path.display()
                )
                .into());
            }
            if name == "core"
                || matches!(name.as_str(), "mem" | "ptr" | "mmio")
                    && import != &format!("core/{name}")
            {
                return Err(
                    format!("import alias `{name}` is reserved for compiler intrinsics").into(),
                );
            }
        }
        let mut imports = program.imports.clone();
        imports.sort();
        imports.dedup();
        for import in imports {
            let import_path = Path::new(&import);
            if import.is_empty()
                || import.contains('\\')
                || import
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
                || import_path
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(format!(
                    "invalid import `{import}` in {}: use a relative package path without `.` or `..`",
                    path.display()
                ).into());
            }
            let import_alias = import_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("invalid import path `{import}`"))?;
            let local_alias = explicit_aliases
                .get(&import)
                .map_or(import_alias, String::as_str);
            let standard = matches!(import.split('/').next(), Some("core" | "alloc" | "std"));
            let dependency = if INTRINSIC_IMPORTS.contains(&import.as_str()) {
                ModuleId::Bundled(import.clone())
            } else if standard {
                let selected = self.native_import(&import)?;
                if !BUNDLED_SOURCES.iter().any(|(name, _)| *name == selected) {
                    return Err(format!(
                        "unknown standard library import `{import}` in {}",
                        path.display()
                    )
                    .into());
                }
                let declared_alias = selected.rsplit('/').next().unwrap().to_owned();
                self.module(ModuleId::Bundled(selected), Some(&declared_alias))?
            } else {
                if matches!(id, ModuleId::Bundled(_)) {
                    return Err(format!(
                        "bundled package {path} cannot import local package `{import}`",
                        path = path.display()
                    )
                    .into());
                }
                let dependency = resolve_import(directory, import_path, self.overlays)?;
                self.module(
                    ModuleId::Local(source_path(&dependency)),
                    Some(import_alias),
                )?
            };
            if let Some(previous) =
                import_aliases.insert(local_alias.to_owned(), dependency.clone())
                && previous != dependency
            {
                return Err(format!(
                    "conflicting package name `{local_alias}` (import alias) in {}; use `import \"path\" as name`",
                    path.display()
                )
                .into());
            }
        }
        self.loading.pop();
        self.modules.insert(
            id.clone(),
            Module {
                sources: source_paths,
                program,
                alias,
                import_aliases,
            },
        );
        Ok(id)
    }
}

/// Canonicalize existing files, while allowing a new unsaved file in an existing directory.
pub fn source_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        match (
            path.parent().and_then(|parent| parent.canonicalize().ok()),
            path.file_name(),
        ) {
            (Some(parent), Some(name)) => parent.join(name),
            _ => path.to_path_buf(),
        }
    })
}

fn source_files(path: &Path, overlays: &BTreeMap<PathBuf, String>) -> Result<Vec<PathBuf>, String> {
    if path.is_file() || overlays.contains_key(path) {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        return Err(format!(
            "{} is neither a source file nor a package directory",
            path.display()
        ));
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path)
        .map_err(|error| format!("cannot read package directory {}: {error}", path.display()))?
    {
        let entry = entry.map_err(|error| {
            format!("cannot read directory entry in {}: {error}", path.display())
        })?;
        if entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?
            .is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "dodo")
        {
            files.push(entry.path());
        }
    }
    files = files.into_iter().map(|file| source_path(&file)).collect();
    files.extend(
        overlays
            .keys()
            .filter(|file| {
                file.parent() == Some(path) && file.extension().is_some_and(|ext| ext == "dodo")
            })
            .cloned(),
    );
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err(format!("{} contains no .dodo source files", path.display()));
    }
    Ok(files)
}

fn resolve_import(
    directory: &Path,
    import: &Path,
    overlays: &BTreeMap<PathBuf, String>,
) -> Result<PathBuf, String> {
    let base = directory.join(import);
    let file = base.with_extension("dodo");
    match (
        file.is_file() || overlays.contains_key(&source_path(&file)),
        base.is_dir(),
    ) {
        (true, false) => Ok(file),
        (false, true) => Ok(base),
        (true, true) => Err(format!(
            "ambiguous import `{}`: both {} and {} exist",
            import.display(),
            file.display(),
            base.display()
        )),
        (false, false) => Err(format!(
            "cannot resolve import `{}`: expected {} or a package directory {}",
            import.display(),
            file.display(),
            base.display()
        )),
    }
}

fn append(target: &mut Program, mut source: Program) {
    target.imports.append(&mut source.imports);
    target.import_aliases.append(&mut source.import_aliases);
    target.structs.append(&mut source.structs);
    target.enums.append(&mut source.enums);
    target.functions.append(&mut source.functions);
    target.constants.append(&mut source.constants);
}

/// Rewrite declarations and their references while preserving local shadowing.
fn namespace(
    program: &mut Program,
    prefix: &str,
    known_packages: &BTreeSet<String>,
    visible_packages: &BTreeMap<String, String>,
) -> Result<(), String> {
    let symbols: BTreeSet<String> = program
        .structs
        .iter()
        .map(|item| item.name.clone())
        .chain(program.enums.iter().map(|item| item.name.clone()))
        .chain(program.constants.iter().map(|item| item.name.clone()))
        .chain(
            program
                .functions
                .iter()
                .map(|item| item.name.split('.').next().unwrap_or(&item.name).to_owned()),
        )
        .collect();
    let names = Names {
        prefix,
        symbols: &symbols,
        known_packages,
        visible_packages,
        missing_imports: RefCell::new(BTreeSet::new()),
    };
    for item in &mut program.structs {
        item.name = names.name(&item.name, &BTreeSet::new());
        let generics = item.generics.iter().cloned().collect();
        for field in &mut item.fields {
            names.ty(&mut field.ty, &generics);
        }
    }
    for item in &mut program.enums {
        item.name = names.name(&item.name, &BTreeSet::new());
        let generics = item.generics.iter().cloned().collect();
        for variant in &mut item.variants {
            for field in &mut variant.fields {
                names.ty(&mut field.ty, &generics);
            }
        }
    }
    for item in &mut program.constants {
        item.name = names.name(&item.name, &BTreeSet::new());
        names.ty(&mut item.ty, &BTreeSet::new());
        names.expr(&mut item.value, &BTreeSet::new(), &BTreeSet::new());
    }
    for function in &mut program.functions {
        function.imported = !prefix.is_empty();
        function.name = names.name(&function.name, &BTreeSet::new());
        let generics = function.generics.iter().cloned().collect();
        let mut locals = BTreeSet::new();
        for parameter in &mut function.params {
            locals.insert(parameter.name.clone());
            names.ty(&mut parameter.ty, &generics);
        }
        names.ty(&mut function.ret, &generics);
        for required in &mut function.requires_plain {
            names.ty(required, &generics);
        }
        if let Some(body) = &mut function.body {
            names.block(body, &mut locals, &generics);
        }
    }
    let missing = names.missing_imports.into_inner();
    if !missing.is_empty() {
        return Err(format!(
            "package `{}` uses package(s) {} without importing them directly",
            program.package,
            missing.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(())
}

struct Names<'a> {
    prefix: &'a str,
    symbols: &'a BTreeSet<String>,
    known_packages: &'a BTreeSet<String>,
    visible_packages: &'a BTreeMap<String, String>,
    missing_imports: RefCell<BTreeSet<String>>,
}
impl Names<'_> {
    fn name(&self, name: &str, excluded: &BTreeSet<String>) -> String {
        // Fully qualified intrinsic calls must obey the same per-package import
        // rules as their shorter `mem`, `ptr`, and `mmio` spellings. `core.drop`
        // remains available without any import.
        if let Some(intrinsic) = name
            .strip_prefix("core.")
            .and_then(|suffix| suffix.split('.').next())
            .filter(|prefix| matches!(*prefix, "mem" | "ptr" | "mmio"))
            && !self
                .visible_packages
                .values()
                .any(|prefix| prefix == intrinsic)
        {
            self.missing_imports
                .borrow_mut()
                .insert(intrinsic.to_owned());
        }
        let first = name.split('.').next().unwrap_or(name);
        if self.symbols.contains(first) && !excluded.contains(first) {
            if self.prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{}.{name}", self.prefix)
            }
        } else {
            if !excluded.contains(first)
                && let Some(prefix) = self.visible_packages.get(first)
            {
                let suffix = name.strip_prefix(first).unwrap();
                return if prefix.is_empty() {
                    suffix.trim_start_matches('.').to_owned()
                } else {
                    format!("{prefix}{suffix}")
                };
            }
            if self.known_packages.contains(first)
                && !self.visible_packages.contains_key(first)
                && !excluded.contains(first)
            {
                self.missing_imports.borrow_mut().insert(first.to_owned());
            }
            name.to_owned()
        }
    }
    fn type_text(&self, text: &str, generics: &BTreeSet<String>) -> String {
        let mut output = String::new();
        let mut cursor = 0;
        while cursor < text.len() {
            let start = cursor;
            let byte = text.as_bytes()[cursor];
            if byte.is_ascii_alphabetic() || byte == b'_' {
                cursor += 1;
                while cursor < text.len()
                    && (text.as_bytes()[cursor].is_ascii_alphanumeric()
                        || matches!(text.as_bytes()[cursor], b'_' | b'.'))
                {
                    cursor += 1;
                }
                output.push_str(&self.name(&text[start..cursor], generics));
            } else {
                let ch = text[cursor..].chars().next().unwrap_or('\0');
                output.push(ch);
                cursor += ch.len_utf8();
            }
        }
        output
    }
    fn length(
        &self,
        length: &mut LengthExpr,
        locals: &BTreeSet<String>,
        generics: &BTreeSet<String>,
    ) {
        match length {
            LengthExpr::Name(name) => *name = self.name(name, locals),
            LengthExpr::Unary(_, v) => self.length(v, locals, generics),
            LengthExpr::Binary(_, a, b) => {
                self.length(a, locals, generics);
                self.length(b, locals, generics);
            }
            LengthExpr::Cast(v, t) => {
                self.length(v, locals, generics);
                self.ty(t, generics);
            }
            LengthExpr::Int(_, Some(t)) => self.ty(t, generics),
            _ => (),
        }
    }
    fn ty(&self, ty: &mut Type, generics: &BTreeSet<String>) {
        self.scoped_ty(ty, &BTreeSet::new(), generics);
    }
    fn scoped_ty(&self, ty: &mut Type, locals: &BTreeSet<String>, generics: &BTreeSet<String>) {
        match ty {
            Type::Named(name) => *name = self.name(name, generics),
            Type::Generic(name, arguments) => {
                *name = self.name(name, generics);
                for ty in arguments {
                    self.scoped_ty(ty, locals, generics);
                }
            }
            Type::ArrayExpr(length, inner) => {
                self.length(length, locals, generics);
                self.scoped_ty(inner, locals, generics);
            }
            Type::Array(_, inner)
            | Type::Slice(_, inner)
            | Type::Ref(_, inner)
            | Type::Raw(_, inner)
            | Type::MaybeUninit(inner)
            | Type::Option(inner) => self.scoped_ty(inner, locals, generics),
            Type::Result(ok, error) => {
                self.scoped_ty(ok, locals, generics);
                self.scoped_ty(error, locals, generics);
            }
            _ => {}
        }
    }
    fn expr(&self, expression: &mut Expr, locals: &BTreeSet<String>, generics: &BTreeSet<String>) {
        self.scoped_ty(&mut expression.ty, locals, generics);
        match &mut expression.kind {
            ExprKind::Name(name) => *name = self.name(name, locals),
            ExprKind::Int(_, Some(ty)) | ExprKind::Float(_, Some(ty)) => {
                self.scoped_ty(ty, locals, generics)
            }
            ExprKind::Array(ty, values) => {
                self.scoped_ty(ty, locals, generics);
                for value in values {
                    self.expr(value, locals, generics);
                }
            }
            ExprKind::Struct(name, fields) => {
                *name = self.type_text(name, &generics.union(locals).cloned().collect());
                for (_, value) in fields {
                    self.expr(value, locals, generics);
                }
            }
            ExprKind::Unary(_, value)
            | ExprKind::Try(value)
            | ExprKind::Unwrap(value)
            | ExprKind::Field(value, _) => self.expr(value, locals, generics),
            ExprKind::Binary(_, left, right)
            | ExprKind::Index(left, right)
            | ExprKind::Range(left, right) => {
                self.expr(left, locals, generics);
                self.expr(right, locals, generics);
            }
            ExprKind::Cast(value, ty)
            | ExprKind::Repeat(value, ty)
            | ExprKind::Constant(value, ty) => {
                self.expr(value, locals, generics);
                self.scoped_ty(ty, locals, generics);
            }
            ExprKind::Call {
                name,
                type_args,
                args,
            } => {
                *name = self.name(name, locals);
                for ty in type_args {
                    self.scoped_ty(ty, locals, generics);
                }
                for argument in args {
                    self.expr(argument, locals, generics);
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.expr(receiver, locals, generics);
                for argument in args {
                    self.expr(argument, locals, generics);
                }
            }
            ExprKind::ValueBlock(body) => self.block(body, &mut locals.clone(), generics),
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expr(base, locals, generics);
                for value in start.iter_mut().chain(end) {
                    self.expr(value, locals, generics);
                }
            }
            _ => {}
        }
    }
    fn pattern(&self, pattern: &mut Pattern, generics: &BTreeSet<String>) {
        match pattern {
            Pattern::Variant(name, fields) => {
                *name = self.type_text(name, generics);
                for field in fields {
                    self.pattern(field, generics);
                }
            }
            Pattern::Struct(name, fields, _) => {
                *name = self.type_text(name, generics);
                for (_, field) in fields {
                    self.pattern(field, generics);
                }
            }
            Pattern::Or(patterns) => {
                for pattern in patterns {
                    self.pattern(pattern, generics);
                }
            }
            _ => {}
        }
    }
    fn block(&self, block: &mut Block, locals: &mut BTreeSet<String>, generics: &BTreeSet<String>) {
        for statement in block {
            self.stmt(statement, locals, generics);
        }
    }
    fn stmt(
        &self,
        statement: &mut Stmt,
        locals: &mut BTreeSet<String>,
        generics: &BTreeSet<String>,
    ) {
        match &mut statement.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                self.scoped_ty(ty, locals, generics);
                if let Some(value) = value {
                    self.expr(value, locals, generics);
                }
                locals.insert(name.clone());
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target, locals, generics);
                self.expr(value, locals, generics);
            }
            StmtKind::LetPattern {
                pattern,
                ty,
                value,
                else_block,
            } => {
                self.scoped_ty(ty, locals, generics);
                self.expr(value, locals, generics);
                if let Some(body) = else_block {
                    self.block(body, &mut locals.clone(), generics);
                }
                self.pattern(pattern, generics);
                locals.extend(pattern.bindings());
            }
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                self.expr(value, locals, generics);
                self.pattern(pattern, generics);
                let mut inner = locals.clone();
                inner.extend(pattern.bindings());
                self.block(then_block, &mut inner, generics);
                self.block(else_block, &mut locals.clone(), generics);
            }
            StmtKind::Expr(value) | StmtKind::Yield(value) | StmtKind::Return(Some(value)) => {
                self.expr(value, locals, generics)
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr(condition, locals, generics);
                self.block(then_block, &mut locals.clone(), generics);
                self.block(else_block, &mut locals.clone(), generics);
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                let mut loop_locals = locals.clone();
                if let Some(init) = init {
                    self.stmt(init, &mut loop_locals, generics);
                }
                if let Some(condition) = condition {
                    self.expr(condition, &loop_locals, generics);
                }
                if let Some(step) = step {
                    self.stmt(step, &mut loop_locals, generics);
                }
                self.block(body, &mut loop_locals, generics);
            }
            StmtKind::ForEach {
                index,
                name,
                iterable,
                body,
                ..
            } => {
                self.expr(iterable, locals, generics);
                let mut loop_locals = locals.clone();
                loop_locals.insert(name.clone());
                if let Some(index) = index {
                    loop_locals.insert(index.clone());
                }
                self.block(body, &mut loop_locals, generics);
            }
            StmtKind::Match { value, arms } => {
                self.expr(value, locals, generics);
                for arm in arms {
                    let mut arm_locals = locals.clone();
                    self.pattern(&mut arm.pattern, generics);
                    arm_locals.extend(arm.pattern.bindings());
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard, &arm_locals, generics);
                    }
                    self.block(&mut arm.body, &mut arm_locals, generics);
                }
            }
            StmtKind::Block(body) | StmtKind::Unsafe(body) => {
                self.block(body, &mut locals.clone(), generics)
            }
            _ => {}
        }
    }
}

fn shift(span: &mut Span, offset: usize) {
    span.start += offset;
    span.end += offset;
}
fn shift_program(program: &mut Program, offset: usize) {
    for item in &mut program.structs {
        shift(&mut item.span, offset);
        for field in &mut item.fields {
            shift(&mut field.span, offset);
        }
    }
    for item in &mut program.enums {
        shift(&mut item.span, offset);
        for variant in &mut item.variants {
            shift(&mut variant.span, offset);
            for field in &mut variant.fields {
                shift(&mut field.span, offset);
            }
        }
    }
    for constant in &mut program.constants {
        shift(&mut constant.span, offset);
        shift_expr(&mut constant.value, offset);
    }
    for function in &mut program.functions {
        shift(&mut function.span, offset);
        shift(&mut function.ret_span, offset);
        if let Some(span) = &mut function.from_span {
            shift(span, offset);
        }
        for parameter in &mut function.params {
            shift(&mut parameter.span, offset);
        }
        if let Some(body) = &mut function.body {
            shift_block(body, offset);
        }
    }
}
fn shift_expr(expression: &mut Expr, offset: usize) {
    shift(&mut expression.span, offset);
    match &mut expression.kind {
        ExprKind::Array(_, values) => {
            for value in values {
                shift_expr(value, offset);
            }
        }
        ExprKind::Struct(_, fields) => {
            for (_, value) in fields {
                shift_expr(value, offset);
            }
        }
        ExprKind::Unary(_, value)
        | ExprKind::Try(value)
        | ExprKind::Unwrap(value)
        | ExprKind::Field(value, _)
        | ExprKind::Cast(value, _)
        | ExprKind::Constant(value, _)
        | ExprKind::Repeat(value, _) => shift_expr(value, offset),
        ExprKind::Binary(_, left, right)
        | ExprKind::Index(left, right)
        | ExprKind::Range(left, right) => {
            shift_expr(left, offset);
            shift_expr(right, offset);
        }
        ExprKind::Call { args, .. } => {
            for argument in args {
                shift_expr(argument, offset);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            shift_expr(receiver, offset);
            for argument in args {
                shift_expr(argument, offset);
            }
        }
        ExprKind::ValueBlock(body) => shift_block(body, offset),
        ExprKind::Slice {
            base, start, end, ..
        } => {
            shift_expr(base, offset);
            for value in start.iter_mut().chain(end) {
                shift_expr(value, offset);
            }
        }
        _ => {}
    }
}
fn shift_block(block: &mut Block, offset: usize) {
    for statement in block {
        shift_stmt(statement, offset);
    }
}
fn shift_stmt(statement: &mut Stmt, offset: usize) {
    shift(&mut statement.span, offset);
    match &mut statement.kind {
        StmtKind::Let {
            value: Some(value), ..
        }
        | StmtKind::Expr(value)
        | StmtKind::Yield(value)
        | StmtKind::Return(Some(value)) => shift_expr(value, offset),
        StmtKind::Assign { target, value, .. } => {
            shift_expr(target, offset);
            shift_expr(value, offset);
        }
        StmtKind::LetPattern {
            value, else_block, ..
        } => {
            shift_expr(value, offset);
            if let Some(body) = else_block {
                shift_block(body, offset);
            }
        }
        StmtKind::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            shift_expr(value, offset);
            shift_block(then_block, offset);
            shift_block(else_block, offset);
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            shift_expr(condition, offset);
            shift_block(then_block, offset);
            shift_block(else_block, offset);
        }
        StmtKind::For {
            init,
            condition,
            step,
            body,
        } => {
            if let Some(init) = init {
                shift_stmt(init, offset);
            }
            if let Some(condition) = condition {
                shift_expr(condition, offset);
            }
            if let Some(step) = step {
                shift_stmt(step, offset);
            }
            shift_block(body, offset);
        }
        StmtKind::ForEach { iterable, body, .. } => {
            shift_expr(iterable, offset);
            shift_block(body, offset);
        }
        StmtKind::Match { value, arms } => {
            shift_expr(value, offset);
            for arm in arms {
                shift(&mut arm.span, offset);
                if let Some(guard) = &mut arm.guard {
                    shift_expr(guard, offset);
                }
                shift_block(&mut arm.body, offset);
            }
        }
        StmtKind::Block(body) | StmtKind::Unsafe(body) => shift_block(body, offset),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dodo-package-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, path: &str, text: &str) {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn loads_local_imports_and_qualifies_internal_symbols() {
        let fixture = Fixture::new();
        fixture.write(
            "main.dodo",
            "package app\nimport \"math\"\nfn main() -> i32 { return math.answer() }\n",
        );
        fixture.write("math.dodo", "package math\nconst i32 VALUE = 42\nfn private() -> i32 { return VALUE }\npub fn answer() -> i32 { return private() }\n");
        let loaded = load(&fixture.0.join("main.dodo")).unwrap();
        assert_eq!(loaded.program.package, "app");
        assert_eq!(loaded.program.constants[0].name, "math.VALUE");
        let answer = loaded
            .program
            .functions
            .iter()
            .find(|function| function.name == "math.answer")
            .unwrap();
        assert!(
            matches!(&answer.body.as_ref().unwrap()[0].kind, StmtKind::Return(Some(Expr { kind: ExprKind::Call { name, .. }, .. })) if name == "math.private")
        );
        assert_eq!(loaded.sources.len(), 2);
    }
    #[test]
    fn directory_packages_combine_sources_and_preserve_local_shadowing() {
        let fixture = Fixture::new();
        fixture.write(
            "main.dodo",
            "package app\nimport \"math\"\nfn main() -> i32 { return math.answer() }\n",
        );
        fixture.write("math/a.dodo", "package math\nconst i32 value = 42\n");
        fixture.write(
            "math/b.dodo",
            "package math\npub fn answer() -> i32 { value := 3\n return value }\n",
        );
        let loaded = load(&fixture.0.join("main.dodo")).unwrap();
        let answer = loaded
            .program
            .functions
            .iter()
            .find(|function| function.name == "math.answer")
            .unwrap();
        assert!(
            matches!(&answer.body.as_ref().unwrap()[1].kind, StmtKind::Return(Some(Expr { kind: ExprKind::Name(name), .. })) if name == "value")
        );
        assert_eq!(loaded.sources.len(), 3);
    }
    #[test]
    fn diagnostics_map_back_to_imported_source() {
        let fixture = Fixture::new();
        fixture.write(
            "main.dodo",
            "package app\nimport \"math\"\nfn main() -> void {}\n",
        );
        fixture.write(
            "math.dodo",
            "package math\npub fn f() -> i32 { return missing }\n",
        );
        let loaded = load(&fixture.0.join("main.dodo")).unwrap();
        let function = loaded
            .program
            .functions
            .iter()
            .find(|function| function.name == "math.f")
            .unwrap();
        let StmtKind::Return(Some(value)) = &function.body.as_ref().unwrap()[0].kind else {
            panic!("return expected");
        };
        let rendered = loaded.render(&Diagnostic::new(value.span, "unknown name"));
        assert!(rendered.contains("math.dodo:2:"), "{rendered}");
        assert!(rendered.contains("return missing"));
    }
    #[test]
    fn diagnostic_labels_resolve_each_file_independently() {
        let fixture = Fixture::new();
        fixture.write(
            "main.dodo",
            "package app\nimport \"views\"\nfn main() -> void {}\n",
        );
        fixture.write(
            "views.dodo",
            "package views\npub fn first(value: &i32) -> &i32 from(value) { return value }\n",
        );
        let loaded = load(&fixture.0.join("main.dodo")).unwrap();
        let main = loaded
            .program
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap();
        let first = loaded
            .program
            .functions
            .iter()
            .find(|function| function.name == "views.first")
            .unwrap();
        let source = loaded
            .sources
            .iter()
            .find(|source| source.path.ends_with("views.dodo"))
            .unwrap();
        let from_span = first.from_span.unwrap();
        assert_eq!(
            &source.text[from_span.start - source.start..from_span.end - source.start],
            "from(value)"
        );
        assert_eq!(
            &source.text[first.ret_span.start - source.start..first.ret_span.end - source.start],
            "-> &i32"
        );
        let rendered = loaded.render(
            &Diagnostic::new(main.span, "invalid borrow")
                .primary_label("borrow escapes here")
                .label(from_span, "borrowed return source declared here"),
        );
        assert!(rendered.contains("main.dodo:3:1"), "{rendered}");
        assert!(rendered.contains("views.dodo:2:"), "{rendered}");
        assert!(rendered.contains("^^^^^^^^"), "{rendered}");
        assert!(
            rendered.contains("----------- borrowed return source declared here"),
            "{rendered}"
        );
        assert_eq!(rendered.matches("error:").count(), 1);
    }
    #[test]
    fn editor_overrides_load_root_and_imports_without_changing_files() {
        let fixture = Fixture::new();
        let disk_root = "package app\nimport \"math\"\nfn main() -> void {}\n";
        let disk_import = "package math\npub fn answer() -> i32 { return 1 }\n";
        fixture.write("main.dodo", disk_root);
        fixture.write("math.dodo", disk_import);
        let root = fixture.0.join("main.dodo").canonicalize().unwrap();
        let import = fixture.0.join("math.dodo").canonicalize().unwrap();
        let root_buffer =
            "package app\nimport \"math\"\nfn main() -> i32 { return math.answer() }\n";
        let import_buffer = "package math\npub fn answer() -> i32 { return 42 }\n";
        let overrides = BTreeMap::from([
            (root.clone(), root_buffer.to_owned()),
            (import.clone(), import_buffer.to_owned()),
        ]);
        let loaded = load_with_overrides(&root, &overrides).unwrap();
        assert_eq!(
            loaded
                .sources
                .iter()
                .find(|source| source.path == root)
                .unwrap()
                .text,
            root_buffer
        );
        assert_eq!(
            loaded
                .sources
                .iter()
                .find(|source| source.path == import)
                .unwrap()
                .text,
            import_buffer
        );
        assert_eq!(std::fs::read_to_string(root).unwrap(), disk_root);
        assert_eq!(std::fs::read_to_string(import).unwrap(), disk_import);
        assert_eq!(
            loaded
                .program
                .functions
                .iter()
                .find(|function| function.name == "main")
                .unwrap()
                .ret,
            Type::Int {
                signed: true,
                bits: 32
            }
        );
    }
    #[test]
    fn rejects_cycles_ambiguous_imports_and_package_mismatches() {
        let fixture = Fixture::new();
        fixture.write("a.dodo", "package a\nimport \"b\"\n");
        fixture.write("b.dodo", "package b\nimport \"a\"\n");
        assert!(
            load(&fixture.0.join("a.dodo"))
                .unwrap_err()
                .contains("cyclic")
        );
        fixture.write("b/b.dodo", "package b\n");
        assert!(
            load(&fixture.0.join("a.dodo"))
                .unwrap_err()
                .contains("ambiguous")
        );
        std::fs::remove_file(fixture.0.join("b.dodo")).unwrap();
        fixture.write("b/b.dodo", "package wrong\n");
        assert!(
            load(&fixture.0.join("a.dodo"))
                .unwrap_err()
                .contains("expected `b`")
        );
    }
    #[test]
    fn root_directory_collects_only_immediate_dodo_files() {
        let fixture = Fixture::new();
        fixture.write("a.dodo", "package app\nfn first() -> void {}\n");
        fixture.write("b.dodo", "package app\nfn second() -> void {}\n");
        fixture.write("ignored.txt", "invalid source");
        fixture.write("sub/nested.dodo", "package other\n");
        assert_eq!(load(&fixture.0).unwrap().program.functions.len(), 2);
    }
    #[test]
    fn transitive_packages_require_a_direct_import() {
        let fixture = Fixture::new();
        fixture.write(
            "main.dodo",
            "package app\nimport \"first\"\nfn main() -> i32 { return second.answer() }\n",
        );
        fixture.write(
            "first.dodo",
            "package first\nimport \"second\"\npub fn answer() -> i32 { return second.answer() }\n",
        );
        fixture.write(
            "second.dodo",
            "package second\npub fn answer() -> i32 { return 42 }\n",
        );
        assert!(
            load(&fixture.0.join("main.dodo"))
                .unwrap_err()
                .contains("without importing")
        );
        fixture.write("main.dodo", "package app\nimport \"first\"\nimport \"second\"\nfn main() -> i32 { return second.answer() }\n");
        assert!(load(&fixture.0.join("main.dodo")).is_ok());
    }
    #[test]
    fn imported_struct_methods_and_generic_literals_are_namespaced() {
        let fixture = Fixture::new();
        fixture.write(
            "main.dodo",
            "package app\nimport \"items\"\nfn main() -> void {}\n",
        );
        fixture.write("items.dodo", "package items\nstruct Item { u8 value\n fn get(self: &Self) -> u8 { return self.value }\n}\nstruct Box<T> { T value }\nfn make() -> Box<Item> { return Box<Item>{value: Item{value: 1}} }\n");
        let loaded = load(&fixture.0.join("main.dodo")).unwrap();
        let method = loaded
            .program
            .functions
            .iter()
            .find(|function| function.name == "items.Item.get")
            .unwrap();
        assert_eq!(
            method.params[0].ty,
            Type::Ref(false, Box::new(Type::Named("items.Item".to_owned())))
        );
        let make = loaded
            .program
            .functions
            .iter()
            .find(|function| function.name == "items.make")
            .unwrap();
        assert!(
            matches!(&make.body.as_ref().unwrap()[0].kind, StmtKind::Return(Some(Expr { kind: ExprKind::Struct(name, _), .. })) if name == "items.Box<items.Item>")
        );
    }
}
