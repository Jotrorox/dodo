//! Portable allocator tests run the same Dodo programs at O0 and O3.
//! The source fixtures are also used by the Windows/Wine validation runner.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const COMPILER: &str = env!("CARGO_BIN_EXE_dodo");
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("dodo-alloc-{}-{id}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn native(source: &str) {
    let workspace = Workspace::new();
    let input = workspace.0.join("test.dodo");
    fs::write(&input, source).unwrap();
    for level in ["0", "3"] {
        let executable = workspace
            .0
            .join(format!("program-O{level}{}", std::env::consts::EXE_SUFFIX));
        let built = Command::new(COMPILER)
            .current_dir(&workspace.0)
            .arg("build")
            .arg(&input)
            .args(["-O", level, "-o"])
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "compile O{level}: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let output = Command::new(executable).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "run O{level}: {:?}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stdout.is_empty(),
            "unexpected output: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

fn rejects(source: &str, expected: &str) {
    let workspace = Workspace::new();
    let input = workspace.0.join("invalid.dodo");
    fs::write(&input, source).unwrap();
    let output = Command::new(COMPILER)
        .arg("check")
        .arg(input)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "invalid allocation program was accepted"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected),
        "expected {expected:?}, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked at"),
        "compiler panicked: {stderr}"
    );
}

#[test]
fn layout_alignment_padding_composition_and_overflow() {
    native(include_str!("stdlib/alloc_layout.dodo"));
}

#[test]
fn arena_alignment_exhaustion_zeroing_reset_and_backing_bounds() {
    native(include_str!("stdlib/alloc_arena.dodo"));
}

#[test]
fn pool_free_list_exhaustion_reuse_and_layout_rejection() {
    native(include_str!("stdlib/alloc_pool.dodo"));
}

#[test]
fn boxed_ownership_drop_failure_cleanup_and_allocator_reuse() {
    native(include_str!("stdlib/alloc_boxed.dodo"));
}

#[test]
fn allocation_layout_invariants_are_private() {
    rejects(
        "package invalid\nimport \"alloc/layout\"\nfn main() { value := layout.Layout { bytes: 1, alignment: 0 } }\n",
        "private",
    );
}

#[test]
fn invalidating_arena_allocations_requires_unsafe() {
    rejects(
        "package invalid\nimport \"alloc/arena\"\nfn main() { bytes := [0u8; 32]\nvalue := arena.Arena.new(&mut bytes)\nvalue.reset() }\n",
        "unsafe",
    );
}

#[test]
fn arena_retains_exclusive_borrow_of_backing_storage() {
    rejects(
        "package invalid\nimport \"alloc/arena\"\nfn main() -> i32 { bytes := [0u8; 32]\nvalue := arena.Arena.new(&mut bytes)\nbytes[0] = 1\nreturn value.capacity() as i32 }\n",
        "borrow",
    );
}

#[test]
fn borrowed_allocator_cannot_be_reset_while_a_box_owns_storage() {
    rejects(
        "package invalid\nimport \"alloc/arena\"\nimport \"alloc/arena_box\"\nimport \"alloc/error\"\nfn attempt() -> void!error.AllocError { bytes := [0u8; 32]\nvalue := arena.Arena.new(&mut bytes)\nowned := arena_box.new(&mut value, 42i32)?\nunsafe { value.reset() }\ncore.drop(owned)\nreturn ok() }\n",
        "borrow",
    );
}

#[test]
fn generic_allocator_contract_requires_unsafe() {
    rejects(
        "package invalid\nimport \"alloc/arena\"\nimport \"alloc/boxed\"\nimport \"alloc/error\"\nfn attempt() -> void!error.AllocError { bytes := [0u8; 32]\nvalue := arena.Arena.new(&mut bytes)\nowned := boxed.new(&mut value, 42i32)?\ncore.drop(owned)\nreturn ok() }\n",
        "unsafe",
    );
}

#[test]
fn boxed_values_cannot_outlive_their_allocator() {
    rejects(
        "package invalid\nimport \"alloc/arena\"\nimport \"alloc/boxed\"\nimport \"alloc/arena_box\"\nimport \"alloc/error\"\nfn escape() -> boxed.Box<i32, arena.Arena>!error.AllocError from(static) { bytes := [0u8; 32]\nvalue := arena.Arena.new(&mut bytes)\nreturn arena_box.new(&mut value, 42i32) }\n",
        "borrow",
    );
}

#[test]
fn boxed_storage_rejects_checked_borrow_payloads() {
    rejects(
        "package invalid\nimport \"alloc/arena\"\nimport \"alloc/arena_box\"\nimport \"alloc/error\"\nstruct Checked { value: &i32 }\nfn attempt() -> void!error.AllocError { bytes := [0u8; 32]\nvalue := arena.Arena.new(&mut bytes)\nnumber := 42i32\nowned := arena_box.new(&mut value, Checked { value: &number })?\ncore.drop(owned)\nreturn ok() }\n",
        "checked",
    );
}

#[test]
fn raw_pool_deallocation_requires_unsafe() {
    rejects(
        "package invalid\nimport \"alloc/pool\"\nimport \"alloc/layout\"\nimport \"alloc/error\"\nfn attempt() -> void!error.AllocError { bytes := [0u8; 32]\nvalue := pool.Pool.new(&mut bytes, 8, 8)?\nrequested := layout.Layout.new(8, 8)?\nallocation := value.allocate(&requested)?\nvalue.deallocate(allocation)\nreturn ok() }\n",
        "unsafe",
    );
}

#[test]
fn safe_box_constructors_cannot_hide_direct_or_nested_results() {
    let cases = [
        ("", "payload: i32!i32 = err(5)"),
        ("", "payload: Option<i32!i32> = some(err(5))"),
        (
            "struct Inner { result: i32!i32 }\nstruct Outer { inner: Inner }",
            "payload := Outer { inner: Inner { result: err(5) } }",
        ),
        (
            "enum Envelope { Payload(i32!i32 result) }",
            "payload := Envelope.Payload(err(5))",
        ),
    ];
    for (declarations, payload) in cases {
        for (allocator, constructor) in [
            ("arena.Arena.new(&mut bytes)", "arena_box.new"),
            ("pool.Pool.new(&mut bytes, 64, 16)?", "pool_box.new"),
        ] {
            rejects(
                &format!(
                    "package invalid\nimport \"alloc/arena\"\nimport \"alloc/pool\"\nimport \"alloc/boxed\"\nimport \"alloc/arena_box\"\nimport \"alloc/pool_box\"\nimport \"alloc/error\"\n{declarations}\nfn attempt() -> void!error.AllocError {{ bytes := [0u8; 256]\nallocator := {allocator}\n{payload}\nowned := {constructor}(&mut allocator, payload)?\ncore.drop(owned)\nreturn ok() }}\n"
                ),
                "unhandled Results",
            );
        }
    }
}

#[test]
fn box_adapters_only_load_the_selected_allocator() {
    for (package, arena, pool) in [
        ("boxed", false, false),
        ("arena_box", true, false),
        ("pool_box", false, true),
    ] {
        let workspace = Workspace::new();
        let input = workspace.0.join("minimal.dodo");
        fs::write(
            &input,
            format!("package minimal\nimport \"alloc/{package}\"\nfn main() {{}}\n"),
        )
        .unwrap();
        let loaded = dodoc::package::load(&input).expect("load minimal Box dependency graph");
        let includes = |name: &str| {
            loaded
                .sources
                .iter()
                .any(|source| source.path.ends_with(format!("{name}.dodo")))
        };
        assert!(
            includes("boxed"),
            "{package} must load the shared Box implementation"
        );
        assert_eq!(includes("arena"), arena, "{package} Arena dependency");
        assert_eq!(includes("pool"), pool, "{package} Pool dependency");
    }
}
