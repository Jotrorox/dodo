//! Embed the source standard library so installed and relocated compilers are
//! independent of the build checkout and the caller's current directory.
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read standard library directory") {
        let entry = entry.expect("read standard library entry");
        let kind = entry.file_type().expect("inspect standard library entry");
        if kind.is_dir() {
            collect(&entry.path(), files);
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "dodo" || ext == "c" || ext == "h")
        {
            files.push(entry.path());
        }
    }
}

fn main() {
    // LLVM's recursive instruction selection can exhaust Windows' 1 MiB
    // default stack when compiling generated codecs, especially at -O0.
    // Reserve address space up front; Windows commits stack pages on demand.
    if env::var("TARGET").is_ok_and(|target| target.ends_with("windows-msvc"))
        && env::var_os("CARGO_FEATURE_LLVM").is_some()
    {
        println!("cargo:rustc-link-arg-bin=dodo=/STACK:8388608");
    }
    // Use the compiler's Rust target, including when cross-compiling Dodo.
    // Frontend package loading must not depend on LLVM to select host adapters.
    println!(
        "cargo:rustc-env=DODO_HOST_TARGET={}",
        env::var("TARGET").unwrap()
    );
    println!("cargo:rerun-if-changed=stdlib");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("stdlib");
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    let mut inventory = String::from("const BUNDLED_SOURCES: &[(&str, &str)] = &[\n");
    let mut native = String::from("const BUNDLED_NATIVE_SOURCES: &[(&str, &str)] = &[\n");
    for file in files {
        let relative = file.strip_prefix(&root).unwrap();
        let name = relative
            .with_extension("")
            .to_str()
            .unwrap()
            .replace('\\', "/");
        assert!(
            name.starts_with("core/") || name.starts_with("alloc/") || name.starts_with("std/"),
            "standard library module must be in core/, alloc/, or std/: {name}"
        );
        let source_path = format!("/stdlib/{}", relative.to_str().unwrap().replace('\\', "/"));
        let entry = format!(
            "    ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {source_path:?}))),\n"
        );
        if file.extension().is_some_and(|ext| ext == "dodo") {
            inventory.push_str(&entry);
        } else {
            let name = relative.to_str().unwrap().replace('\\', "/");
            native.push_str(&format!("    ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {source_path:?}))),\n"));
        }
    }
    inventory.push_str("];\n");
    native.push_str("];\n");
    inventory.push_str(&native);
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("stdlib_sources.rs");
    fs::write(output, inventory).expect("write embedded standard library inventory");
}
