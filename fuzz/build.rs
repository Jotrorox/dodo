//! With the `image` feature, compile the std/image fuzz targets
//! (fuzz/image/libfuzzer.dodo) into a static library the image_* fuzz
//! targets link. The Dodo compiler emits LLVM bitcode, which clang compiles
//! with libFuzzer's coverage instrumentation (-fsanitize=fuzzer-no-link) so
//! libFuzzer is guided by coverage of the Dodo code itself.
//!
//! Environment:
//!   DODO                 the dodo compiler (default: ../target/release/dodo,
//!                        then ../target/debug/dodo)
//!   DODO_FUZZ_CLANG      the clang that instruments the bitcode; it must read
//!                        the bitcode of dodo's LLVM (default: clang next to
//!                        `llvm-config --bindir`, then clang on PATH)
//!   DODO_FUZZ_OPT        dodo optimization level (default 1)
//!   DODO_FUZZ_NO_COVERAGE=1  link dodo's object file without instrumentation
//!                        (libFuzzer then only sees coverage of the Rust glue)
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    for name in [
        "DODO",
        "DODO_FUZZ_CLANG",
        "DODO_FUZZ_OPT",
        "DODO_FUZZ_NO_COVERAGE",
        "AR",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_IMAGE").is_none() {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let repository = manifest.parent().unwrap().to_path_buf();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dodo = find_dodo(&repository);
    println!("cargo:rerun-if-changed={}", dodo.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("image").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repository.join("stdlib/std/image").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repository.join("stdlib/std/image.dodo").display()
    );

    let opt = std::env::var("DODO_FUZZ_OPT").unwrap_or_else(|_| "1".to_string());
    let object = out.join("dodo_image.o");
    let source = manifest.join("image/libfuzzer.dodo");
    if std::env::var_os("DODO_FUZZ_NO_COVERAGE").is_some() {
        println!("cargo:warning=DODO_FUZZ_NO_COVERAGE: the Dodo code is not coverage-instrumented");
        dodo_compile(&dodo, &source, "obj", &opt, &object);
    } else {
        let bitcode = out.join("dodo_image.bc");
        dodo_compile(&dodo, &source, "bitcode", &opt, &bitcode);
        let clang = find_clang();
        let status = Command::new(&clang)
            .args([
                "-c",
                "-O1",
                "-fsanitize=fuzzer-no-link",
                "-Wno-override-module",
            ])
            .arg(&bitcode)
            .arg("-o")
            .arg(&object)
            .status()
            .unwrap_or_else(|e| panic!("cannot run {}: {e}", clang.display()));
        assert!(
            status.success(),
            "{} could not instrument the Dodo bitcode. It must be the clang of the LLVM version \
             dodo was built with (set DODO_FUZZ_CLANG), or set DODO_FUZZ_NO_COVERAGE=1 to link \
             without coverage instrumentation.",
            clang.display()
        );
    }
    let library = out.join("libdodo_image.a");
    let _ = std::fs::remove_file(&library);
    let ar = std::env::var("AR").unwrap_or_else(|_| "ar".to_string());
    let status = Command::new(&ar)
        .arg("crs")
        .arg(&library)
        .arg(&object)
        .status()
        .unwrap_or_else(|e| panic!("cannot run {ar}: {e}"));
    assert!(
        status.success(),
        "{ar} failed to archive {}",
        object.display()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=dodo_image");
}

fn find_dodo(repository: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("DODO") {
        let path = PathBuf::from(path);
        assert!(path.is_file(), "DODO={} does not exist", path.display());
        return path;
    }
    for profile in ["release", "debug"] {
        let path = repository
            .join("target")
            .join(profile)
            .join(format!("dodo{}", std::env::consts::EXE_SUFFIX));
        if path.is_file() {
            return path;
        }
    }
    panic!(
        "the image fuzz targets need the dodo compiler: run `cargo build` (or `cargo build --release`) \
         in the repository root first, or set DODO=/path/to/dodo"
    );
}

fn dodo_compile(dodo: &Path, source: &Path, emit: &str, opt: &str, output: &Path) {
    let mut command = Command::new(dodo);
    command
        .args(["build", "--emit", emit, "-O", opt, "-o"])
        .arg(output)
        .arg(source);
    let target = std::env::var("TARGET").unwrap();
    if target != std::env::var("HOST").unwrap() {
        command.args(["--target", &target]);
    }
    let result = command
        .output()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", dodo.display()));
    assert!(
        result.status.success(),
        "{} build {} failed:\n{}\n{}",
        dodo.display(),
        source.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn find_clang() -> PathBuf {
    if let Some(path) = std::env::var_os("DODO_FUZZ_CLANG") {
        return PathBuf::from(path);
    }
    // llvm-sys's prefix variable, llvm-config, then the usual install
    // locations of LLVM 23 (the version dodo links), then PATH.
    let mut candidates = Vec::new();
    if let Some(prefix) = std::env::var_os("LLVM_SYS_231_PREFIX") {
        candidates.push(PathBuf::from(prefix).join("bin/clang"));
    }
    if let Ok(output) = Command::new("llvm-config").arg("--bindir").output()
        && output.status.success()
    {
        let bin = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        candidates.push(bin.join("clang"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/opt/llvm@23/bin/clang"));
    candidates.push(PathBuf::from("/usr/local/opt/llvm@23/bin/clang"));
    candidates.push(PathBuf::from("/usr/lib/llvm-23/bin/clang"));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("clang"))
}
