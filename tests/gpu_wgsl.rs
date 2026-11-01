//! std/gpu's `gpu.wgsl`: shaders written once in WGSL and translated at
//! compile time. The fixture runs on whatever GPU the machine has (Metal on
//! macOS; Vulkan and OpenGL elsewhere, where Mesa's lavapipe and llvmpipe
//! are enough) and exits 2 without one; it builds for every hosted target;
//! and mistakes in a shader are compile errors pointing into the literal.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HOSTED_TARGETS: [&str; 6] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-pc-windows-gnu",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("dodo-wgsl-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
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

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture() -> PathBuf {
    root().join("tests/stdlib/gpu_wgsl_checks.dodo")
}

/// Mipmaps, shadow maps, depth bias, multisampling, several color targets
/// and custom blending, with WGSL shaders.
fn feature_fixture() -> PathBuf {
    root().join("tests/stdlib/gpu_feature_checks.dodo")
}

/// The diagnostics of checking `source`, which must fail.
fn check_fails(scratch: &Scratch, source: &str) -> String {
    let path = scratch.file("main.dodo", source);
    let output = dodo(&["check", path.to_str().unwrap()]);
    assert!(!output.status.success(), "the check passed:\n{source}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Build `source` at -O`optimization` and run it: it must pass on every
/// backend it finds and end with `ok_line`, or exit 2 without a backend.
fn run_fixture(source: &Path, optimization: &str, ok_line: &str) {
    let scratch = Scratch::new("checks");
    let program = scratch
        .0
        .join(format!("checks{}", std::env::consts::EXE_SUFFIX));
    success(
        &dodo(&[
            "build",
            source.to_str().unwrap(),
            "-O",
            optimization,
            "-o",
            program.to_str().unwrap(),
        ]),
        &format!("compile {}", source.display()),
    );
    let output = Command::new(&program).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    match output.status.code() {
        Some(0) => {
            assert!(stdout.ends_with(ok_line), "{stdout}");
            if cfg!(target_os = "macos") {
                assert!(stdout.contains("Metal ok"), "{stdout}");
            }
        }
        Some(2) => {
            assert!(stdout.ends_with("no backend\n"), "{stdout}");
            eprintln!("no GPU backend: the shaders were only translated");
        }
        _ => panic!(
            "{}: {}\n{stdout}\n{}",
            source.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

#[test]
fn wgsl_checks_run_on_every_available_backend() {
    run_fixture(&fixture(), "0", "wgsl ok\n");
}

#[test]
fn feature_checks_run_on_every_available_backend() {
    for optimization in ["0", "3"] {
        run_fixture(&feature_fixture(), optimization, "features ok\n");
    }
}

#[test]
fn wgsl_checks_build_for_every_hosted_target() {
    let scratch = Scratch::new("targets");
    for (name, source) in [("checks", fixture()), ("features", feature_fixture())] {
        for target in HOSTED_TARGETS {
            let object = scratch.0.join(format!("{name}-{target}.o"));
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
                &format!("emit {} for {target}", source.display()),
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
}

/// The line and column a diagnostic points at, from its `--> path:line:col`.
fn location(stderr: &str) -> (usize, usize) {
    let arrow = stderr
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("--> "))
        .unwrap_or_else(|| panic!("no location:\n{stderr}"));
    let mut parts = arrow.rsplitn(3, ':');
    let column = parts.next().unwrap().parse().unwrap();
    let line = parts.next().unwrap().parse().unwrap();
    (line, column)
}

#[test]
fn shader_mistakes_point_into_the_literal() {
    let scratch = Scratch::new("diagnostics");
    // An unknown name, on the second literal.
    let stderr = check_fails(
        &scratch,
        "package main\nimport \"std/gpu\"\n\nfn main() -> i32 {\n    source := gpu.wgsl(\n        \"@compute @workgroup_size(1)\",\n        \"fn go() { let x: u32 = nonsense; }\"\n    )\n    return source.spirv.len as i32\n}\n",
    );
    assert!(stderr.contains("invalid WGSL shader"), "{stderr}");
    assert!(stderr.contains("nonsense"), "{stderr}");
    assert_eq!(location(&stderr), (7, 33), "{stderr}");
    // A syntax error.
    let stderr = check_fails(
        &scratch,
        "package main\nimport \"std/gpu\"\n\nfn main() -> i32 {\n    source := gpu.wgsl(\"@compute @workgroup_size(1) fn go() { let = 1; }\")\n    return source.spirv.len as i32\n}\n",
    );
    assert!(stderr.contains("invalid WGSL shader"), "{stderr}");
    assert_eq!(location(&stderr).0, 5, "{stderr}");
    // A reserved Metal name points at the entry point.
    let stderr = check_fails(
        &scratch,
        "package main\nimport \"std/gpu\"\n\nfn main() -> i32 {\n    source := gpu.wgsl(\n        \"@compute @workgroup_size(1)\",\n        \"fn main() {}\"\n    )\n    return source.spirv.len as i32\n}\n",
    );
    assert!(stderr.contains("reserved name in Metal"), "{stderr}");
    assert_eq!(location(&stderr), (7, 13), "{stderr}");
    // std/gpu's binding convention.
    let stderr = check_fails(
        &scratch,
        "package main\nimport \"std/gpu\"\n\nfn main() -> i32 {\n    source := gpu.wgsl(\"@group(0) @binding(0) var s: sampler; @compute @workgroup_size(1) fn go() { _ = s; }\")\n    return source.spirv.len as i32\n}\n",
    );
    assert!(stderr.contains("@group(1)"), "{stderr}");
}

#[test]
fn wgsl_takes_literals_only() {
    let scratch = Scratch::new("literals");
    let stderr = check_fails(
        &scratch,
        "package main\nimport \"std/gpu\"\n\nfn main() -> i32 {\n    text := \"@compute @workgroup_size(1) fn go() {}\"\n    source := gpu.wgsl(text)\n    return source.spirv.len as i32\n}\n",
    );
    assert!(stderr.contains("string literals"), "{stderr}");
    let stderr = check_fails(
        &scratch,
        "package main\nimport \"std/gpu\"\n\nfn main() -> i32 {\n    translate := gpu.wgsl\n    return 0\n}\n",
    );
    assert!(stderr.contains("compile time"), "{stderr}");
}

#[test]
fn compiler_wgsl_is_reserved_for_std_gpu() {
    let scratch = Scratch::new("reserved");
    let source: &Path = &scratch.file(
        "main.dodo",
        "package main\n\n@compiler(wgsl)\npub fn shader(source: &str) -> i32\n\nfn main() -> i32 {\n    return 0\n}\n",
    );
    let output = dodo(&["check", source.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("reserved for std/gpu"), "{stderr}");
}
