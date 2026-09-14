//! Checked collection-region effects, supported shared elements, and rejected obligations.
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
fn fixed_vectors_and_exclusive_or_result_allocated_elements_remain_restricted() {
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
        if element == "&mut i32" || diagnostic == "Result" {
            workspace.rejects(
            &format!("package app\nimport \"alloc/shared_arena\"\nimport \"std/collections/shared_vector\"\n{declarations}\nfn construct(allocator: shared_arena.Handle) {{ values := shared_vector.new::<{element}>(allocator) }}"),
            diagnostic,
        );
        }
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

fn allocated_program(body: &str, helpers: &str) -> String {
    format!(
        r#"package app
import "alloc/error"
import "alloc/shared_arena"
import "std/collections/vector"
import "std/collections/shared_vector"
import "std/text_shared"
struct Wrapper {{ values: vector.Vector<&i32, shared_arena.Handle> }}
{helpers}
fn run() -> void!error.AllocError {{
    bytes := [0u8; 4096]
    arena := shared_arena.SharedArena.new(&mut bytes)?
    source := 42i32
    second := 7i32
    values := shared_vector.new::<&i32>(arena.handle())
    {body}
    return ok()
}}
"#
    )
}

#[test]
fn stored_sources_survive_effects_aliases_moves_removal_clear_and_joins() {
    let w = Workspace::new();
    for body in [
        "values.push(&source)?\nsource = 0",
        "values.push(&source)?\nremoved := values.pop()\ncore.drop(values)\nsource = 0\ncore.drop(removed)",
        "values.push(&source)?\nvalues.clear()\nsource = 0",
        "values.push(&source)?\nmoved := values\nsource = 0\ncore.drop(moved)",
        "core.drop(values.replace(0, &source))\nsource = 0",
        "match values.push(&source) { ok() => {}, err(_) => {} }\nsource = 0",
        "alias := &mut values\nalias.push(&source)?\nremoved := alias.pop()\ncore.drop(values)\nsource = 0\ncore.drop(removed)",
        "wrapper := Wrapper { values: values }\nwrapper.values.push(&source)?\nsource = 0",
        "if second == 7 { values.push(&source)? } else { values.push(&second)? }\nsource = 0",
        "for { if second == 7 { values.push(&source)?\nbreak } else { break } }\nsource = 0",
        "for second < 9 { source = 0\nvalues.push(&source)? }",
        "for i in 0..2 { if i == 0 { values.push(&source)?\ncontinue }\nsource = 0 }",
        "{ short := 3i32\nvalues.push(&short)? }",
        "alias := &mut values\n{ short := 3i32\nalias.push(&short)? }\ncore.drop(alias.pop())",
        "values.push(&source)?\nview := values.get(0)\nvalues.clear()\ncore.drop(view)",
        "values.push(&source)?\nview := values.as_slice()\nvalues.reserve(100)?\ncore.drop(view)",
        "values.push(&source)?\nview := values.get(0)\ncore.drop(values)\ncore.drop(view)",
    ] {
        w.rejects(&allocated_program(body, ""), "borrow");
    }
}

#[test]
fn external_effects_and_stored_return_contracts_are_verified() {
    let w = Workspace::new();
    for helpers in [
        "fn put(out: &mut vector.Vector<&i32, shared_arena.Handle>, value: &i32) -> void!error.AllocError { return out.push(value) }",
        "fn put(out: &mut vector.Vector<&i32, shared_arena.Handle>, value: &i32) -> void!error.AllocError stores(out, value) { local := 3i32\nreturn out.push(&local) }",
        "fn put(out: &mut vector.Vector<&i32, shared_arena.Handle>, value: &i32, other: &i32) -> void!error.AllocError stores(out, value) { return out.push(other) }",
        "fn put(out: &mut vector.Vector<&i32, shared_arena.Handle>, value: &i32) -> Option<&i32>!error.AllocError from(out.stored) stores(out, value) { out.push(value)?\nreturn ok(out.pop()) }",
    ] {
        w.rejects(&allocated_program("", helpers), "source");
    }
    w.rejects(&allocated_program("", "fn view(out: &vector.Vector<&i32, shared_arena.Handle>) -> &[&i32] from(out.stored) { return out.as_slice() }"), "return contract");
    w.rejects(
        &allocated_program("values.push(&source)?\ncore.drop(values.get_mut(0))", ""),
        "requires plain elements",
    );
    w.rejects(
        &allocated_program(
            "values.push(&source)?\ncore.drop(values.as_mut_slice())",
            "",
        ),
        "requires plain elements",
    );
    w.rejects(&allocated_program("", "fn escape(a: shared_arena.Handle) -> vector.Vector<&i32, shared_arena.Handle>!error.AllocError from(a) { x := 3i32\nv := shared_vector.new::<&i32>(a)\nv.push(&x)?\nreturn ok(v) }"), "borrow");
}

#[test]
fn owned_text_sources_and_allocation_obligations_cannot_be_discarded() {
    let w = Workspace::new();
    for body in [
        "text := text_shared.from_str(arena.handle(), \"abc\", 100)?\ntexts := shared_vector.new::<&str>(arena.handle())\ntexts.push(text.as_str())?\ntext.clear()",
        "text := text_shared.from_str(arena.handle(), \"abc\", 100)?\ntexts := shared_vector.new::<&str>(arena.handle())\ntexts.push(text.as_str())?\ncore.drop(text)",
        "texts := shared_vector.new::<text_shared.String>(arena.handle())\ntexts.push(text_shared.from_str(arena.handle(), \"abc\", 100)?)?\ncore.drop(arena)",
        "texts := shared_vector.new::<text_shared.String>(arena.handle())\ntexts.push(text_shared.from_str(arena.handle(), \"abc\", 100)?)?\nremoved := texts.pop()\ncore.drop(texts)\nunsafe { arena.reset() }\ncore.drop(removed)",
    ] {
        w.rejects(&allocated_program(body, ""), "borrow");
    }
    for body in [
        "values.push(&source)",
        "core.drop(values.insert(0, &source))",
        "_ = text_shared.from_str(arena.handle(), \"abc\", 100)",
        "pending := text_shared.new(arena.handle(), 100)",
    ] {
        w.rejects(&allocated_program(body, ""), "Result");
    }
}

#[test]
fn shared_reference_collections_and_owned_text_execute_at_o0_and_o3() {
    let w = Workspace::new();
    for fixture in ["reference_collections", "shared_text_collections"] {
        for level in ["0", "3"] {
            let exe =
                w.0.join(format!("{fixture}-{level}{}", std::env::consts::EXE_SUFFIX));
            let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/stdlib")
                        .join(format!("{fixture}.dodo")),
                )
                .args(["-O", level, "-o"])
                .arg(&exe)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{fixture} O{level}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = Command::new(exe).output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(0),
                "{fixture} O{level}: {output:?}"
            );
        }
    }
}

#[test]
fn typed_storage_primitives_require_unsafe_matching_owners_and_effects() {
    let w = Workspace::new();
    for (body, diagnostic) in [
        (
            "fn take(p: *mut &i32, owner: &mut Region) -> &i32 from(owner.stored) { return ptr.take(p, owner) }",
            "requires an explicit unsafe",
        ),
        (
            "fn take(p: *mut &i32, owner: &mut u8) -> &i32 from(owner) { unsafe { return ptr.take(p, owner) } }",
            "lacks a zero-length witness",
        ),
        (
            "fn take(p: *const &i32, owner: &mut Region) -> &i32 from(owner.stored) { unsafe { return ptr.take(p, owner) } }",
            "mutable raw pointer",
        ),
        (
            "fn take(p: *mut &i32, owner: &Region) -> &i32 from(owner.stored) { unsafe { return ptr.take(p, owner) } }",
            "exclusive owner borrow",
        ),
        (
            "fn put(p: *mut &i32, owner: &mut Region, value: &i32) { unsafe { ptr.store(p, value, owner) } }",
            "stores(target, source",
        ),
        (
            "fn view(p: *mut &i32, owner: &Region) -> & &i32 from(owner.stored) { unsafe { return ptr.view(p, owner) } }",
            "return contract",
        ),
        (
            "fn invalid(owner: &mut Region, value: &i32) stores(missing, value) {}",
            "stores target",
        ),
        (
            "fn invalid(owner: &mut Region, value: &i32) stores(owner, missing) {}",
            "stores source",
        ),
    ] {
        w.rejects(
            &format!(
                "package app\nimport \"core/ptr\"\nstruct Region {{ witness: [0]&i32 }}\n{body}"
            ),
            diagnostic,
        );
    }
}

#[test]
fn reference_to_result_collections_cannot_discharge_a_union_of_obligations() {
    let w = Workspace::new();
    w.rejects(r#"package app
import "alloc/error"
import "alloc/shared_arena"
import "std/collections/shared_vector"
fn run() -> void!error.AllocError {
    bytes := [0u8; 4096]
    arena := shared_arena.SharedArena.new(&mut bytes)?
    first: i32!u8 = ok(1)
    second: i32!u8 = err(2)
    values := shared_vector.new::<&Result<i32, u8>>(arena.handle())
    match values.push(&first) {ok() => {}, err(_) => {}}
    match values.push(&second) {ok() => {}, err(_) => {}}
    removed := values.pop()
    core.drop(values)
    match removed { some(value) => { match value {ok(_) => {}, err(_) => {}} }, none => { match &first {ok(_) => {}, err(_) => {}}
match &second {ok(_) => {}, err(_) => {}} } }
    return ok()
}
"#, "Result");
    for element in [
        "&Result<i32, u8>",
        "&Wrapped",
        "Option<&Wrapped>",
        "Borrowed",
    ] {
        w.rejects(&format!("package app\nimport \"alloc/shared_arena\"\nimport \"std/collections/shared_vector\"\nstruct Wrapped {{ result: i32!u8 }}\nstruct Borrowed {{ inner: &Wrapped }}\nfn check(a: shared_arena.Handle) {{ v := shared_vector.new::<{element}>(a) }}"), "Result");
    }
}

#[test]
fn parameter_moves_and_projections_cannot_erase_stored_sources() {
    let w = Workspace::new();
    let vector = "vector.Vector<&i32, shared_arena.Handle>";
    let declarations = format!(
        "struct OwnedWrapper {{ values: {vector} }}\nstruct Indirect {{ values: &mut {vector} }}\nenum Wrapped {{ Values({vector}), Empty }}"
    );
    for (parameter, body) in [
        (format!("values: {vector}"), "return values.pop()"),
        ("owner: OwnedWrapper".into(), "return owner.values.pop()"),
        ("owner: Indirect".into(), "return owner.values.pop()"),
        (format!("values: [1]{vector}"), "return values[0].pop()"),
        (format!("values: &mut[{vector}]"), "return values[0].pop()"),
        (
            format!("values: Option<{vector}>"),
            "match values { some(value) => { return value.pop() }, none => { return none } }",
        ),
        (
            format!("values: Result<{vector}, u8>"),
            "match values { ok(value) => { return value.pop() }, err(_) => { return none } }",
        ),
        (
            "owner: Wrapped".into(),
            "match owner { Wrapped.Values(value) => { return value.pop() }, Wrapped.Empty => { return none } }",
        ),
        (
            "owner: &mut Wrapped".into(),
            "match owner { Wrapped.Values(value) => { return value.pop() }, Wrapped.Empty => { return none } }",
        ),
    ] {
        w.rejects(
            &allocated_program("", &format!("{declarations}\nfn erase({parameter}) -> Option<&i32> from(static) {{ {body} }}")),
            "return contract",
        );
    }
    w.rejects(&allocated_program("", "fn erase(values: vector.Vector<text_shared.String, shared_arena.Handle>) -> Option<text_shared.String> from(static) { return values.pop() }"), "return contract");
    w.rejects(&allocated_program("", "fn stale(values: vector.Vector<&i32, shared_arena.Handle>) -> &[&i32] from(values) { return values.as_slice() }"), "borrow");
    w.rejects(
        &allocated_program(
            "values.push(&source)?\nremoved := consume(values)\nsource = 0\ncore.drop(removed)",
            "fn consume(values: vector.Vector<&i32, shared_arena.Handle>) -> Option<&i32> from(values) { return values.pop() }",
        ),
        "borrow",
    );
}
