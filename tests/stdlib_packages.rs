//! Bundled packages must work with an installed compiler and ordinary packages.
use dodoc::{ast::Type, diagnostic::Diagnostic, package};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        loop {
            let path = std::env::temp_dir().join(format!(
                "dodo-stdlib-packages-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create workspace: {error}"),
            }
        }
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        path
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn copied_compiler_loads_bundled_sources_from_an_unrelated_directory() {
    let workspace = Workspace::new();
    let compiler = workspace
        .0
        .join(format!("installed-dodo{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_dodo"), &compiler).unwrap();
    let source = workspace.write(
        "project/main.dodo",
        "package app\nimport \"core/ascii\"\nfn main() -> i32 {\n if ascii.is_digit(55) { return 0 }\n return 1\n}\n",
    );
    fs::create_dir(workspace.0.join("unrelated")).unwrap();
    let output = Command::new(&compiler)
        .current_dir(workspace.0.join("unrelated"))
        .arg("check")
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn local_packages_can_import_stdlib_and_stdlib_dependencies_are_deduplicated() {
    let workspace = Workspace::new();
    let source = workspace.write(
        "main.dodo",
        "package app\nimport \"helper\"\nimport \"core/ascii\"\nimport \"alloc/arena\"\nfn main() -> bool { return helper.answer() }\n",
    );
    workspace.write(
        "helper.dodo",
        "package helper\nimport \"core/ascii\"\npub fn answer() -> bool { return ascii.is_digit(55) }\n",
    );
    let loaded = package::load(&source).unwrap();
    for import in ["core/ascii", "alloc/arena", "alloc/layout", "alloc/error"] {
        assert!(
            loaded.program.imports.iter().any(|name| name == import),
            "missing {import}"
        );
        let suffix = format!("{import}.dodo");
        assert_eq!(
            loaded
                .sources
                .iter()
                .filter(|source| source.path.ends_with(&suffix))
                .count(),
            1
        );
    }
    assert!(
        loaded
            .program
            .functions
            .iter()
            .any(|function| function.name == "ascii.is_digit")
    );
    assert!(
        loaded
            .program
            .functions
            .iter()
            .any(|function| function.name == "helper.answer")
    );
}

#[test]
fn bundled_imports_cannot_be_shadowed_by_local_files_or_editor_overlays() {
    let workspace = Workspace::new();
    let source = workspace.write(
        "main.dodo",
        "package app\nimport \"core/ascii\"\nfn main() -> bool { return ascii.is_digit(55) }\n",
    );
    let shadow = workspace.write("core/ascii.dodo", "this is deliberately invalid Dodo");
    let overrides = BTreeMap::from([
        (package::source_path(&shadow), "invalid overlay".to_owned()),
        (
            PathBuf::from("<stdlib>/core/ascii.dodo"),
            "invalid synthetic overlay".to_owned(),
        ),
    ]);
    let loaded = package::load_with_overlays(&source, &overrides).unwrap();
    assert!(
        loaded
            .sources
            .iter()
            .all(|source| !source.text.contains("invalid"))
    );
    assert!(
        loaded
            .program
            .functions
            .iter()
            .any(|function| function.name == "ascii.is_digit")
    );
}

#[test]
fn unknown_and_malformed_stdlib_imports_are_rejected_without_local_fallback() {
    let workspace = Workspace::new();
    workspace.write("core/missing.dodo", "package missing\n");
    workspace.write("alloc/missing.dodo", "package missing\n");
    for import in ["core", "alloc", "core/missing", "alloc/missing"] {
        let source = workspace.write("main.dodo", &format!("package app\nimport \"{import}\"\n"));
        let error = package::load(&source).unwrap_err();
        assert!(error.contains("unknown standard library import"), "{error}");
        assert!(error.contains("main.dodo"), "{error}");
    }
    for import in [
        "core/./mem",
        "core/../mem",
        "core//mem",
        "core/mem/",
        "/core/mem",
    ] {
        let source = workspace.write("main.dodo", &format!("package app\nimport \"{import}\"\n"));
        let error = package::load(&source).unwrap_err();
        assert!(error.contains("invalid import"), "{error}");
    }
}

#[test]
fn local_packages_cannot_conflict_with_bundled_or_intrinsic_aliases() {
    let workspace = Workspace::new();
    let source = workspace.write("main.dodo", "package core\n");
    assert!(
        package::load(&source)
            .unwrap_err()
            .contains("reserved for compiler intrinsics")
    );
    for alias in ["mem", "ptr", "mmio"] {
        let source = workspace.write("main.dodo", &format!("package {alias}\n"));
        let error = package::load(&source).unwrap_err();
        assert!(error.contains("conflicting package name"), "{error}");
    }
    for imports in [
        "import \"ascii\"\nimport \"core/ascii\"",
        "import \"core/ascii\"\nimport \"ascii\"",
    ] {
        workspace.write(
            "ascii.dodo",
            "package ascii\npub fn is_digit(value: u8) -> bool { return false }\n",
        );
        let source = workspace.write("main.dodo", &format!("package app\n{imports}\n"));
        let error = package::load(&source).unwrap_err();
        assert!(
            error.contains("conflicting package name `ascii`"),
            "{error}"
        );
    }
    let source = workspace.write("main.dodo", "package ascii\nimport \"core/ascii\"\n");
    assert!(
        package::load(&source)
            .unwrap_err()
            .contains("conflicting package name `ascii`")
    );
}

#[test]
fn transitive_imports_do_not_grant_source_or_intrinsic_package_visibility() {
    let workspace = Workspace::new();
    workspace.write(
        "helper.dodo",
        "package helper\nimport \"core/ascii\"\nimport \"core/mem\"\npub fn answer() -> bool { return ascii.is_digit(55) }\n",
    );
    for expression in [
        "ascii.is_digit(55)",
        "mem.size_of<u8>()",
        "core.mem.size_of<u8>()",
    ] {
        let source = workspace.write(
            "main.dodo",
            &format!(
                "package app\nimport \"helper\"\nfn main() -> void {{ value := {expression}\n }}\n"
            ),
        );
        let error = package::load(&source).unwrap_err();
        assert!(error.contains("without importing them directly"), "{error}");
    }
    let source = workspace.write("main.dodo", "package app\nimport \"core/mem\"\nfn main() -> void { value := core.mem.size_of<u8>()\n core.drop(value)\n }\n");
    assert!(package::load(&source).is_ok());
}

#[test]
fn bundled_diagnostics_keep_source_text_and_synthetic_paths() {
    let workspace = Workspace::new();
    let source = workspace.write("main.dodo", "package app\nimport \"core/ascii\"\n");
    let loaded = package::load(&source).unwrap();
    let function = loaded
        .program
        .functions
        .iter()
        .find(|function| function.name == "ascii.is_digit")
        .unwrap();
    assert_eq!(function.ret, Type::Bool);
    let rendered = loaded.render(&Diagnostic::new(function.span, "test diagnostic"));
    assert!(rendered.contains("ascii.dodo:"), "{rendered}");
    assert!(rendered.contains("<stdlib>"), "{rendered}");
    assert!(rendered.contains("is_digit"), "{rendered}");
}

#[test]
fn ordinary_source_overlays_are_preserved_with_bundled_dependencies() {
    let workspace = Workspace::new();
    let source = workspace.write("main.dodo", "package app\ninvalid source\n");
    let buffer =
        "package app\nimport \"core/ascii\"\nfn main() -> bool { return ascii.is_digit(55) }\n";
    let loaded = package::load_with_overlays(
        &source,
        &BTreeMap::from([(package::source_path(&source), buffer.to_owned())]),
    )
    .unwrap();
    assert_eq!(loaded.sources[0].text, buffer);
    assert!(
        fs::read_to_string(source)
            .unwrap()
            .contains("invalid source")
    );
}

#[test]
fn uninitialized_storage_preserves_imported_type_namespaces() {
    let workspace = Workspace::new();
    let source = workspace.write("main.dodo", "package app\nimport \"storage\"\n");
    workspace.write(
        "storage.dodo",
        "package storage\npub struct Item { pub value: u8 }\npub struct Slot { pub value: MaybeUninit<Item> }\n",
    );
    let loaded = package::load(&source).unwrap();
    let slot = loaded
        .program
        .structs
        .iter()
        .find(|item| item.name == "storage.Slot")
        .unwrap();
    assert_eq!(
        slot.fields[0].ty,
        Type::MaybeUninit(Box::new(Type::Named("storage.Item".to_owned())))
    );
}
