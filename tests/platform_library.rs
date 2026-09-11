//! Hosted adapter selection must not infect portable foundation dependencies.
use dodoc::package;
use std::fs;

#[test]
fn adapters_are_selected_independently_and_reject_wrong_targets() {
    let scratch = std::env::temp_dir().join(format!("dodo-platform-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for area in ["fs", "process", "env", "thread", "sync"] {
        let source = scratch.join(format!("{area}.dodo"));
        fs::write(
            &source,
            format!("package fixture\nimport \"std/{area}\"\nfn main() {{}}\n"),
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
