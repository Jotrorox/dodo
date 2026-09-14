//! Portable byte I/O contracts, progress semantics, and checked adapter borrows.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-std-io-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
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
fn portable_io_executes_at_both_optimization_levels_and_cross_compiles() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Workspace::new();
    for fixture in ["std_io", "std_io_alloc", "std_line_io", "io_bounded"] {
        let source = root.join(format!("tests/stdlib/{fixture}.dodo"));
        for optimization in ["0", "3"] {
            let executable = scratch.0.join(format!(
                "{fixture}-O{optimization}{}",
                std::env::consts::EXE_SUFFIX
            ));
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
                    .arg(&executable)
                    .current_dir(&scratch.0)
                    .output()
                    .unwrap(),
                &format!("compile {fixture} at O{optimization}"),
            );
            success(
                Command::new(&executable).output().unwrap(),
                &format!("execute {fixture} at O{optimization}"),
            );
            for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
                let object = scratch
                    .0
                    .join(format!("{fixture}-{target}-O{optimization}.o"));
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
                            optimization,
                            "-o",
                        ])
                        .arg(&object)
                        .output()
                        .unwrap(),
                    &format!("cross compile {fixture} for {target} at O{optimization}"),
                );
                assert!(fs::metadata(object).unwrap().len() > 0);
            }
        }
    }
    let source = root.join("examples/io.dodo");
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("run")
            .arg(source)
            .output()
            .unwrap(),
        "execute documented I/O example",
    );
}

#[test]
fn checked_io_views_and_result_obligations_are_preserved() {
    let scratch = Workspace::new();
    let cases = [
        (
            "escaping-reader",
            "fn escape() -> io.MemoryReader from(static) {\n data := [1u8]\n return io.MemoryReader.new(&data)\n}\n",
            "borrow",
        ),
        (
            "written-view",
            "fn main() -> void {\n data := [0u8; 4]\n writer := io.MemoryWriter.new(&mut data)\n input := [1u8]\n view := writer.written()\n match writer.write(&input) { ok(_) => {} err(_) => {} }\n value := view.len\n}\n",
            "borrow",
        ),
        (
            "buffer-storage",
            "fn main() -> void {\n data := [1u8]\n storage := [0u8; 4]\n reader := io.MemoryReader.new(&data)\n match io.BufferedReader.new(&mut reader, &mut storage) {\n ok(buffered) => { storage[0] = 2\n value := buffered.buffered().len\n }\n err(_) => {}\n }\n}\n",
            "borrow",
        ),
        (
            "pending-view",
            "fn main() -> void {\n data := [0u8; 4]\n storage := [0u8; 2]\n writer := io.MemoryWriter.new(&mut data)\n match io.BufferedWriter.new(&mut writer, &mut storage) {\n ok(buffered) => { view := buffered.pending()\n match buffered.flush() { ok(_) => {} err(_) => {} }\n value := view.len\n }\n err(_) => {}\n }\n}\n",
            "borrow",
        ),
        (
            "ignored-error",
            "fn main() -> void {\n data := [0u8; 4]\n writer := io.MemoryWriter.new(&mut data)\n input := [1u8]\n result := io.write_all(&mut writer, &input)\n}\n",
            "Result",
        ),
    ];
    for (name, body, expected) in cases {
        let source = scratch.0.join(format!("{name}.dodo"));
        fs::write(
            &source,
            format!("package safety\nimport \"std/io\"\n{body}"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{name} unexpectedly accepted");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains(expected), "{name}: {diagnostic}");
    }
}
