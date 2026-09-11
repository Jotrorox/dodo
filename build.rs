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
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "dodo") {
            files.push(entry.path());
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=stdlib");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("stdlib");
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    let mut inventory = String::from("const BUNDLED_SOURCES: &[(&str, &str)] = &[\n");
    for file in files {
        let relative = file.strip_prefix(&root).unwrap();
        let name = relative
            .with_extension("")
            .to_str()
            .unwrap()
            .replace('\\', "/");
        assert!(
            name.starts_with("core/") || name.starts_with("alloc/"),
            "standard library module must be in core/ or alloc/: {name}"
        );
        let source_path = format!("/stdlib/{}", relative.to_str().unwrap().replace('\\', "/"));
        inventory.push_str(&format!(
            "    ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {source_path:?}))),\n"
        ));
    }
    inventory.push_str("];\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("stdlib_sources.rs");
    fs::write(output, inventory).expect("write embedded standard library inventory");
}
