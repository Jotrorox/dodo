//! Metal through the Objective-C runtime: code generation for both macOS
//! architectures and rejection elsewhere everywhere; on macOS the GPU
//! fixture at O0 and O3 (and as x86-64 under Rosetta), every constant
//! against the SDK headers, and the binding-index checks.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("dodo-metal-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn dodo(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn metal_builds_for_macos_and_is_rejected_elsewhere() {
    let scratch = Scratch::new("targets");
    let source = root().join("tests/stdlib/gpu_metal_checks.dodo");
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        let object = scratch.0.join(format!("checks-{target}.o"));
        success(
            &dodo(&[
                "build",
                source.to_str().unwrap(),
                "--emit",
                "obj",
                "--target",
                target,
                "-o",
                object.to_str().unwrap(),
            ]),
            &format!("emit the Metal fixture for {target}"),
        );
        assert!(fs::metadata(object).unwrap().len() > 0);
    }
    for import in ["std/gpu/metal", "std/gpu/metal/objc"] {
        let program = scratch.0.join("import.dodo");
        fs::write(
            &program,
            format!("package main\nimport \"{import}\"\nfn main() {{}}\n"),
        )
        .unwrap();
        for target in [
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "wasm32-unknown-unknown",
            "thumbv6m-none-eabi",
        ] {
            let output = dodo(&["check", program.to_str().unwrap(), "--target", target]);
            assert!(!output.status.success(), "{import} accepted for {target}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("Metal requires a macOS target"),
                "{import} for {target}: {stderr}"
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn run_checks(executable: &Path, launcher: Option<&str>) {
    let output = match launcher {
        Some(arch) => Command::new("arch").arg(arch).arg(executable).output(),
        None => Command::new(executable).output(),
    }
    .unwrap();
    success(&output, &format!("run {}", executable.display()));
    if String::from_utf8_lossy(&output.stdout).contains("skipped") {
        eprintln!("no Metal device: only the device-independent checks ran");
    }
}

/// Builds without link arguments: loading std/gpu/metal must add the
/// frameworks by itself.
#[cfg(target_os = "macos")]
#[test]
fn metal_checks_run_on_the_gpu() {
    let scratch = Scratch::new("gpu");
    let source = root().join("tests/stdlib/gpu_metal_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!("checks-O{optimization}"));
        success(
            &dodo(&[
                "build",
                source.to_str().unwrap(),
                "-O",
                optimization,
                "-o",
                executable.to_str().unwrap(),
            ]),
            &format!("build the Metal fixture -O{optimization}"),
        );
        run_checks(&executable, None);
    }
    // x86-64 returns MTLSize through objc_msgSend_stret and classifies the
    // structs differently; Rosetta runs it on Apple silicon.
    let rosetta = cfg!(target_arch = "x86_64")
        || Command::new("arch")
            .args(["-x86_64", "/usr/bin/true"])
            .status()
            .is_ok_and(|status| status.success());
    if rosetta {
        let executable = scratch.0.join("checks-x86_64");
        success(
            &dodo(&[
                "build",
                source.to_str().unwrap(),
                "--target",
                "x86_64-apple-darwin",
                "-o",
                executable.to_str().unwrap(),
            ]),
            "build the Metal fixture for x86-64",
        );
        run_checks(&executable, Some("-x86_64"));
    }
}

/// Binding indices past Metal's argument tables panic before reaching it.
#[cfg(target_os = "macos")]
#[test]
fn binding_indices_are_checked() {
    let scratch = Scratch::new("indices");
    let program = scratch.0.join("index.dodo");
    for (call, fails) in [
        ("encoder.set_buffer(&buffer, 0, 30)", false),
        ("encoder.set_buffer(&buffer, 0, 31)", true),
        ("encoder.set_bytes(&bytes, 31)", true),
        ("encoder.set_threadgroup_memory_length(16, 31)", true),
    ] {
        fs::write(
            &program,
            format!(
                "package main\nimport \"std/gpu/metal\"\n\
                 fn run() -> void!metal.Error {{\n\
                 device := metal.Device.system_default()?\n\
                 queue := device.new_command_queue()?\n\
                 buffer := device.new_buffer(16, metal.RESOURCE_STORAGE_MODE_SHARED)?\n\
                 bytes := [0u8; 4]\n\
                 commands := queue.command_buffer()?\n\
                 encoder := commands.compute_command_encoder()?\n\
                 {call}\n\
                 encoder.end_encoding()\n\
                 return ok()\n}}\n\
                 fn main() -> i32 {{\n\
                 match run() {{\n\
                 ok() => {{ return 0 }}\n\
                 err(metal.Error.NoDevice) => {{ return 3 }}\n\
                 err(_) => {{ return 1 }}\n}}\n}}\n"
            ),
        )
        .unwrap();
        let executable = scratch.0.join("index");
        success(
            &dodo(&[
                "build",
                program.to_str().unwrap(),
                "-o",
                executable.to_str().unwrap(),
            ]),
            &format!("build {call}"),
        );
        let output = Command::new(&executable).output().unwrap();
        if output.status.code() == Some(3) {
            eprintln!("no Metal device: binding indices not checked");
            return;
        }
        assert_eq!(!output.status.success(), fails, "{call}: {output:?}");
    }
}

/// Normalized for matching: no MTL prefix or underscores, upper case.
#[cfg(target_os = "macos")]
fn normalized(name: &str) -> String {
    name.trim_start_matches("MTL")
        .replace('_', "")
        .to_uppercase()
}

/// Every public constant in std/gpu/metal names a Metal enumerator by its
/// spelling, and has that enumerator's value in the installed SDK.
#[cfg(target_os = "macos")]
#[test]
fn constants_match_the_sdk_headers() {
    let Ok(sdk) = Command::new("xcrun").arg("--show-sdk-path").output() else {
        eprintln!("xcrun unavailable: constants not checked");
        return;
    };
    let sdk = PathBuf::from(String::from_utf8_lossy(&sdk.stdout).trim());
    let headers = sdk.join("System/Library/Frameworks/Metal.framework/Headers");
    let Ok(entries) = fs::read_dir(&headers) else {
        eprintln!(
            "no Metal headers in {}: constants not checked",
            sdk.display()
        );
        return;
    };
    let mut identifiers = std::collections::BTreeMap::new();
    for entry in entries {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
        for word in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            if word.starts_with("MTL") && word.len() > 3 {
                identifiers.insert(normalized(word), word.to_owned());
            }
        }
    }
    let source = fs::read_to_string(root().join("stdlib/std/gpu/metal.dodo")).unwrap();
    let mut program =
        String::from("#import <Metal/Metal.h>\n#include <stdio.h>\nint main(void) {\n");
    let mut expected = Vec::new();
    for line in source.lines() {
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let (name, rest) = rest.split_once(':').unwrap();
        let value = rest.rsplit_once('=').unwrap().1.trim();
        // Binding limits are this module's own, not Metal enumerators.
        if name.starts_with("MAX_") {
            continue;
        }
        let metal = identifiers
            .get(&normalized(name))
            .unwrap_or_else(|| panic!("{name} matches no Metal identifier"));
        program.push_str(&format!(
            "printf(\"%llu\\n\", (unsigned long long){metal});\n"
        ));
        expected.push((name.to_owned(), value.parse::<u64>().unwrap()));
    }
    program.push_str("return 0;\n}\n");
    assert!(
        expected.len() > 300,
        "only {} constants found",
        expected.len()
    );
    let scratch = Scratch::new("constants");
    let file = scratch.0.join("constants.m");
    let executable = scratch.0.join("constants");
    fs::write(&file, program).unwrap();
    success(
        &Command::new("cc")
            .args(["-x", "objective-c", "-w"])
            .arg(&file)
            .args(["-framework", "Metal", "-framework", "Foundation", "-o"])
            .arg(&executable)
            .output()
            .unwrap(),
        "compile the constant table",
    );
    let output = Command::new(&executable).output().unwrap();
    success(&output, "print the constant table");
    let actual: Vec<u64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.parse().unwrap())
        .collect();
    assert_eq!(actual.len(), expected.len());
    for ((name, value), actual) in expected.iter().zip(actual) {
        assert_eq!(*value, actual, "{name}");
    }
}
