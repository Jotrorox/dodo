//! Portable algorithms, ownership-moving containers, and model-based fixtures.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-collections-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const FIXTURES: &[&str] = &[
    "collections_algorithms",
    "collections_fixed",
    "collections_owned",
    "collections_destruction",
];
fn native(fixture: &str) {
    let workspace = Workspace::new();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/stdlib")
        .join(format!("{fixture}.dodo"));
    for optimization in ["0", "3"] {
        let executable = workspace.0.join(format!("{fixture}-{optimization}"));
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(["-O", optimization, "-o"])
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{fixture} O{optimization}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "{fixture} O{optimization}: {:?} {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
fn borrowed_algorithms_match_deterministic_models() {
    native(FIXTURES[0]);
}
#[test]
fn bounded_containers_preserve_order_and_capacity() {
    native(FIXTURES[1]);
}
#[test]
fn owned_containers_collisions_growth_destruction_and_models() {
    native(FIXTURES[2]);
}
#[test]
fn move_only_elements_drop_exactly_once_across_container_operations() {
    native(FIXTURES[3]);
}
#[test]
fn portable_collections_emit_wasm_and_cortex_m0_objects() {
    let workspace = Workspace::new();
    for fixture in FIXTURES {
        for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
            let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/stdlib")
                .join(format!("{fixture}.dodo"));
            let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["-O", "3", "--emit", "obj", "--target", target, "-o"])
                .arg(workspace.0.join(format!("{fixture}-{target}.o")))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{fixture} {target}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
fn rejects(body: &str, message: &str) {
    let workspace = Workspace::new();
    let source = workspace.0.join("main.dodo");
    fs::write(&source, format!(r#"package invalid
import "alloc/error"
import "alloc/shared_arena"
import "std/collections/shared_vector"
import "std/collections/shared_hash_map"
pub struct BorrowedPolicy {{
    value: &i32
    pub fn hash(&self, key: &i32) -> u64 {{ return *self.value as u64 }}
    pub fn equal(&self, a: &i32, b: &i32) -> bool {{ return *a == *b }}
}}
fn verify() -> i32!error.AllocError {{
    bytes := [0u8; 4096]
    allocator := shared_arena.SharedArena.new(&mut bytes)?
    {body}
    return ok(0)
}}
fn main() -> i32 {{ match verify() {{ ok(code) => {{ return code }}, err(_) => {{ return 99 }}, }} }}
"#)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("check")
        .arg(source)
        .output()
        .unwrap();
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "invalid ownership accepted: {body}"
    );
    assert!(
        diagnostic.contains(message),
        "expected {message}: {diagnostic}"
    );
    assert!(!diagnostic.contains("panicked at"), "{diagnostic}");
}
#[test]
fn owned_views_prevent_mutation_and_owner_destruction() {
    rejects(
        "values := shared_vector.new::<i32>(allocator.handle())\nvalues.push(1)?\nview := values.as_slice()\nvalues.push(2)?\nreturn ok(view[0])",
        "conflict",
    );
    rejects(
        "values := shared_vector.new::<i32>(allocator.handle())\nvalues.push(1)?\nview := values.as_slice()\ncore.drop(values)\nreturn ok(view[0])",
        "borrow",
    );
    rejects(
        "values := shared_vector.new::<i32>(allocator.handle())\ncore.drop(allocator)\nvalues.push(1)?",
        "borrow",
    );
}
#[test]
fn owned_elements_cannot_hide_borrows_or_result_obligations() {
    rejects(
        "values := shared_vector.new::<&i32>(allocator.handle())",
        "borrow",
    );
    rejects(
        "values := shared_vector.new::<Option<i32!u8>>(allocator.handle())",
        "Result",
    );
}

#[test]
fn policies_preserve_checked_dependencies_alongside_shared_allocators() {
    rejects(
        "salt := 1i32\nmap := shared_hash_map.new::<i32, i32, BorrowedPolicy>(allocator.handle(), BorrowedPolicy { value: &salt })\nsalt = 2\ncore.drop(map.insert(1, 2)?)",
        "conflict",
    );
}
