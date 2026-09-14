//! Characterization of the checker boundary described in container-elements.md.
//! Rejected safe-looking helpers are intentional blockers, not enabled features.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-container-elements-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn rejects(&self, source: &str, diagnostic: &str) {
        let path = self.0.join("rejected.dodo");
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(path)
            .output()
            .unwrap();
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted:\n{source}");
        assert!(error.contains(diagnostic), "{source}\n{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fixed_and_allocated_vectors_keep_recursive_element_restrictions() {
    let workspace = Workspace::new();
    for (element, diagnostic) in [
        ("&i32", "borrow"),
        ("&mut i32", "borrow"),
        ("&str", "borrow"),
        ("&[i32]", "borrow"),
        ("Option<&i32>", "borrow"),
        ("[1]&i32", "borrow"),
        ("Borrowed", "borrow"),
        ("BorrowedEnum", "borrow"),
        ("i32!u8", "Result"),
        ("Option<i32!u8>", "Result"),
        ("[1]Result<i32, u8>", "Result"),
        ("Outcome", "Result"),
        ("OutcomeEnum", "Result"),
    ] {
        let declarations = "struct Borrowed { value: Option<&i32> }\nenum BorrowedEnum { Value(Borrowed), Empty }\nstruct Outcome { value: Option<i32!u8> }\nenum OutcomeEnum { Value(Outcome), Empty }";
        // A borrowed parameter avoids a separate pending Result on the caller's
        // slot array, so this failure comes from the container specialization.
        workspace.rejects(
            &format!("package app\nimport \"std/collections/fixed_vector\"\n{declarations}\nfn construct(slots: &mut[Option<{element}>]) {{ values := fixed_vector.Vector.new(slots) }}"),
            diagnostic,
        );
        workspace.rejects(
            &format!("package app\nimport \"alloc/shared_arena\"\nimport \"std/collections/shared_vector\"\n{declarations}\nfn construct(allocator: shared_arena.Handle) {{ values := shared_vector.new::<{element}>(allocator) }}"),
            diagnostic,
        );
    }
}

#[test]
fn fixed_reference_insertion_and_extraction_need_separate_checker_effects() {
    let workspace = Workspace::new();
    for helper in [
        "fn put(slots: &mut[Option<&i32>], value: &i32) { slots[0] = some(value) }",
        "fn put(slot: &mut Option<&i32>, value: &i32) { *slot = some(value) }",
        "fn put(slot: &mut Option<&i32>) { local := 7i32\n*slot = some(&local) }",
        // Even removing all dependencies needs a write effect on the caller.
        "fn clear(slot: &mut Option<&i32>) { *slot = none }",
        "struct Holder { slot: Option<&i32> }\nfn put(out: &mut Holder, value: &i32) { out.slot = some(value) }",
    ] {
        workspace.rejects(
            &format!("package app\n{helper}"),
            "replacing borrow-carrying fields through a reference",
        );
    }
    workspace.rejects(
        "package app\nimport \"core/option\"\nfn take(slot: &mut Option<&i32>) -> Option<&i32> from(slot) { return option.take(slot) }",
        "replacing or swapping checked-borrow values is unsupported",
    );
    workspace.rejects(
        "package app\nimport \"core/mem\"\nfn replace(slot: &mut Option<&i32>, value: &i32) -> Option<&i32> from(slot, value) { return mem.replace(slot, some(value)) }",
        "replacing or swapping checked-borrow values is unsupported",
    );
    workspace.rejects(
        "package app\nimport \"core/option\"\nfn take(slot: &mut Option<i32!u8>) -> Option<i32!u8> { return option.take(slot) }",
        "Result-containing values is unsupported",
    );
}

#[test]
fn visible_aggregate_moves_keep_reference_sources_live() {
    let workspace = Workspace::new();
    for body in [
        "x := 1i32\nslot := Holder { value: some(&x) }\nmoved := slot\nx = 2\ncore.drop(moved)",
        "slot: Option<&i32> = none\n{ x := 1i32\nslot = some(&x) }\ncore.drop(slot)",
        "x := 1i32\ny := 2i32\nslot := Holder { value: some(&x) }\nslot.value = some(&y)\ny = 3\ncore.drop(slot)",
        // The source must survive a destructor even without an explicit use.
        "x := 1i32\nslot := WithDrop { value: &x }\nx = 2",
        "x := 1i32\nslot: Option<&mut i32> = some(&mut x)\nother := &mut x\nmatch slot { some(p) => { *p = 2 }, none => {} }\n*other = 3",
    ] {
        workspace.rejects(
            &format!("package app\nstruct Holder {{ value: Option<&i32> }}\nstruct WithDrop {{ value: &i32\nfn drop(&mut self) {{ observed := *self.value }} }}\nfn main() {{ {body} }}"),
            "borrow",
        );
    }
    workspace.rejects(
        "package app\nfn escape() -> Option<&i32> from(static) { x := 1i32\nreturn some(&x) }",
        "borrow",
    );
}

#[test]
fn views_keep_storage_and_allocator_lifetimes_through_container_mutation() {
    let workspace = Workspace::new();
    for mutation in [
        "values.push(2)?",
        "core.drop(values.pop())",
        "core.drop(values.replace(0, 2))",
        "values.reserve(64)?",
        "values.clear()",
        "core.drop(values)",
        "moved := values",
        "core.drop(allocator)",
        "unsafe { allocator.reset() }",
        "bytes[0] = 1",
    ] {
        workspace.rejects(
            &format!("package app\nimport \"alloc/error\"\nimport \"alloc/shared_arena\"\nimport \"std/collections/shared_vector\"\nfn test() -> i32!error.AllocError {{ bytes := [0u8; 4096]\nallocator := shared_arena.SharedArena.new(&mut bytes)?\nvalues := shared_vector.new::<i32>(allocator.handle())\nvalues.push(1)?\nview := values.as_slice()\n{mutation}\nreturn ok(view[0]) }}"),
            "borrow",
        );
    }
    for mutation in ["slots[0] = some(2)", "values.clear()", "core.drop(values)"] {
        workspace.rejects(
            &format!("package app\nimport \"std/collections\"\nimport \"std/collections/fixed_vector\"\nfn test() -> i32!collections.CapacityError {{ slots: [1]Option<i32> = [none]\nvalues := fixed_vector.Vector.new(&mut slots)\nvalues.push(1)?\nview := values.get(0)\n{mutation}\nmatch view {{ some(value) => {{ return ok(*value) }}, none => {{ return ok(0) }} }} }}"),
            "borrow",
        );
    }
}

#[test]
fn visible_reference_and_result_aggregates_destroy_active_payloads_once() {
    let workspace = Workspace::new();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/stdlib/container_element_baseline.dodo");
    for level in ["0", "3"] {
        let executable = workspace
            .0
            .join(format!("baseline-O{level}{}", std::env::consts::EXE_SUFFIX));
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(["-O", level, "-o"])
            .arg(&executable)
            .output()
            .unwrap();
        assert!(output.status.success(), "O{level}: {output:?}");
        let output = Command::new(executable).output().unwrap();
        assert_eq!(output.status.code(), Some(0), "O{level}: {output:?}");
    }
}
