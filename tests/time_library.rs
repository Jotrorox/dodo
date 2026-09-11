//! Exact time arithmetic, Gregorian/calendar models, fake capabilities and text.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn time_boundaries_models_and_portable_objects() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-time-library-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = root.join("tests/stdlib/time_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!(
            "time-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
                .arg(&executable)
                .current_dir(&scratch)
                .output()
                .unwrap(),
            "compile time fixture",
        );
        success(
            Command::new(&executable).output().unwrap(),
            "execute time fixture",
        );
    }
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        let object = scratch.join(format!("time-{target}.o"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args([
                    "build",
                    source.to_str().unwrap(),
                    "--emit",
                    "obj",
                    "--target",
                    target,
                    "-O",
                    "3",
                    "-o",
                ])
                .arg(&object)
                .output()
                .unwrap(),
            &format!("emit time freestanding object for {target}"),
        );
        assert!(fs::metadata(object).unwrap().len() > 0);
    }
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn time_invariants_and_result_handling_are_checked() {
    let scratch = std::env::temp_dir().join(format!("dodo-time-reject-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for (name, body, expected) in [
        (
            "private",
            "value := time.Duration { secs: 0, nanos: 1000000000 }",
            "private",
        ),
        ("result", "time.Duration.new(0, 0)", "Result"),
        (
            "different_types",
            "instant := time.Instant.new(1, time.Duration.zero())\nstamp := time.Timestamp.epoch()\nmatch instant.duration_since(&stamp) { ok(_) => {}, err(_) => {} }",
            "type",
        ),
    ] {
        let source = scratch.join(format!("{name}.dodo"));
        fs::write(
            &source,
            format!("package rejected\nimport \"std/time\"\nfn main() {{\n{body}\n}}\n"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {name}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.to_lowercase().contains(&expected.to_lowercase()),
            "{name}: {diagnostic}"
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
