//! Hosted adapter selection must not infect portable foundation dependencies.
use dodoc::package;
use std::fs;

#[cfg(all(
    target_arch = "x86_64",
    target_pointer_width = "64",
    any(
        all(target_os = "linux", target_env = "gnu"),
        all(target_os = "windows", any(target_env = "msvc", target_env = "gnu"))
    )
))]
#[test]
fn default_loaders_select_host_adapters_without_llvm() {
    let scratch = std::env::temp_dir().join(format!("dodo-host-adapter-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = scratch.join("main.dodo");
    fs::write(&source, "package app\nimport \"std/fs\"\nfn main() {}\n").unwrap();
    let adapter = if cfg!(windows) { "windows" } else { "linux" };
    let loaded = package::load(&source).unwrap();
    assert!(
        loaded
            .sources
            .iter()
            .any(|s| s.path.ends_with(format!("fs/{adapter}.dodo")))
    );

    let overlays = std::collections::BTreeMap::from([(
        package::source_path(&source),
        "package app\nimport \"std/env\"\nfn main() {}\n".to_owned(),
    )]);
    let loaded = package::load_with_overlays(&source, &overlays).unwrap();
    assert!(
        loaded
            .sources
            .iter()
            .any(|s| s.path.ends_with(format!("env/{adapter}.dodo")))
    );
    assert!(
        loaded
            .sources
            .iter()
            .all(|s| !s.path.ends_with(format!("fs/{adapter}.dodo")))
    );
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn adapters_are_selected_independently_and_reject_wrong_targets() {
    let scratch = std::env::temp_dir().join(format!("dodo-platform-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for area in ["fs", "process", "env", "thread", "sync", "time"] {
        let source = scratch.join(format!("{area}.dodo"));
        let import = if area == "time" { "time/hosted" } else { area };
        fs::write(
            &source,
            format!("package fixture\nimport \"std/{import}\"\nfn main() {{}}\n"),
        )
        .unwrap();
        for (target, adapter) in [
            ("x86_64-unknown-linux-gnu", "linux"),
            ("x86_64-pc-windows-msvc", "windows"),
        ] {
            let loaded = package::load_for_target(&source, target).unwrap();
            assert!(
                loaded
                    .sources
                    .iter()
                    .any(|s| s.path.ends_with(format!("{area}/{adapter}.dodo")))
            );
            for other in ["fs", "process", "env", "thread", "sync"] {
                if other != area {
                    assert!(
                        !loaded
                            .sources
                            .iter()
                            .any(|s| s.path.ends_with(format!("{other}/{adapter}.dodo"))),
                        "std/{area} unexpectedly imports std/{other}"
                    );
                }
            }
        }
        for target in [
            "thumbv6m-none-eabi",
            "wasm32-unknown-unknown",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "x86_64-unknown-linux-gnux32",
        ] {
            let error = package::load_for_target(&source, target).unwrap_err();
            assert!(error.contains("unsupported for target"), "{error}");
        }
    }
    let source = scratch.join("wrong.dodo");
    fs::write(
        &source,
        "package wrong\nimport \"std/fs/linux\"\nfn main() {}\n",
    )
    .unwrap();
    assert!(
        package::load_for_target(&source, "x86_64-pc-windows-msvc")
            .unwrap_err()
            .contains("cannot be used")
    );
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn portable_atomic_contract_has_no_native_blocking_dependency() {
    let scratch = std::env::temp_dir().join(format!("dodo-atomic-portable-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = scratch.join("portable.dodo");
    fs::write(&source, "package portable\nimport \"std/sync/atomic\"\nimport \"core/num\"\nimport \"alloc/arena\"\nimport \"std/io\"\nimport \"std/text\"\nimport \"std/time\"\nfn main() {}\n").unwrap();
    for target in [
        "x86_64-unknown-linux-gnu",
        "thumbv6m-none-eabi",
        "wasm32-unknown-unknown",
    ] {
        let loaded = package::load_for_target(&source, target).unwrap();
        assert!(package::native_sources(&loaded).is_empty());
    }
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn protocol_imports_remain_portable_and_backends_link_independently() {
    let scratch =
        std::env::temp_dir().join(format!("dodo-protocol-imports-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = scratch.join("portable.dodo");
    fs::write(&source, "package portable\nimport \"std/net\"\nimport \"std/net/dns\"\nimport \"std/tls\"\nimport \"std/http\"\nimport \"std/web\"\nimport \"std/web/server\"\nimport \"std/web/application\"\nimport \"std/web/response\"\nfn main() {}\n").unwrap();
    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "thumbv6m-none-eabi",
        "wasm32-unknown-unknown",
    ] {
        let loaded = package::load_for_target(&source, target).unwrap();
        assert!(package::native_sources(&loaded).is_empty(), "{target}");
    }
    for area in ["net", "tls", "web"] {
        fs::write(
            &source,
            format!("package backend\nimport \"std/{area}/native\"\nfn main() {{}}\n"),
        )
        .unwrap();
        for target in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"] {
            let loaded = package::load_for_target(&source, target).unwrap();
            let native = package::native_sources(&loaded);
            let names: Vec<_> = native.iter().map(|(name, _)| *name).collect();
            let expected = if area == "net" {
                vec!["std/net/runtime.c", "std/time/runtime.c"]
            } else if area == "tls" {
                vec!["std/tls/runtime.c"]
            } else {
                vec!["std/web/runtime.c"]
            };
            assert_eq!(names, expected, "{area} for {target}");
        }
        assert!(
            package::load_for_target(&source, "thumbv6m-none-eabi")
                .unwrap_err()
                .contains("unsupported")
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
