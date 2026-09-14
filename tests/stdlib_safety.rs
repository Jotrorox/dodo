//! Independent safety regressions for the primitives used by the alloc library.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Source(PathBuf);

impl Source {
    fn new(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-stdlib-safety-{}-{}.dodo",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, source).unwrap();
        Self(path)
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn rejects(source: &str, message: &str) {
    let source = Source::new(source);
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("check")
        .arg(&source.0)
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "invalid program was accepted:\n{}",
        fs::read_to_string(&source.0).unwrap()
    );
    assert!(error.contains(message), "expected {message:?}:\n{error}");
    assert!(
        !error.contains("panicked at"),
        "compiler panicked:\n{error}"
    );
}

#[test]
fn opaque_storage_cannot_hide_checked_borrows_or_unhandled_results() {
    for (declarations, expression) in [
        ("value := 1u8", "&value"),
        ("value: u8!u8 = err(2)", "value"),
        ("value: Option<u8!u8> = some(err(2))", "value"),
        (
            "value := 1u8\nwrapped := Borrowed { value: &value }",
            "wrapped",
        ),
    ] {
        rejects(
            &format!(
                "package app\nimport \"core/mem\"\nstruct Borrowed {{ value: &u8 }}\nfn main() -> i32 {{\n{declarations}\nstorage := mem.init({expression})\nreturn 0\n}}\n"
            ),
            "cannot hide checked borrows or unhandled Results",
        );
    }
}

#[test]
fn extracting_opaque_storage_requires_unsafe_and_cannot_invent_checked_borrows() {
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { storage := mem.init(1u8)\nvalue := mem.assume_init(storage)\nreturn value as i32\n}\n",
        "requires an explicit unsafe block",
    );
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { storage := mem.uninit::<&u8>()\nunsafe { value := mem.assume_init(storage)\nreturn *value as i32\n}\n}\n",
        "reading checked-borrow values from opaque storage is unsupported",
    );
}

#[test]
fn opaque_storage_is_moved_and_initialization_consumes_its_payload() {
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { storage := mem.uninit::<u8>()\nother := storage\ncore.drop(storage)\nreturn 0\n}\n",
        "cannot use `storage` after it was moved",
    );
    rejects(
        "package app\nimport \"core/mem\"\nstruct Item { value: i32 }\nfn main() -> i32 { item := Item { value: 7 }\nstorage := mem.init(item)\nreturn item.value\n}\n",
        "cannot use `item` after it was moved",
    );
    rejects(
        "package app\nimport \"core/mem\"\nimport \"core/ptr\"\nstruct Item { value: i32 }\nfn main() -> i32 { storage := mem.uninit::<Item>()\nitem := Item { value: 7 }\nunsafe { ptr.write(mem.uninit_as_mut_ptr(&mut storage), item) }\nreturn item.value\n}\n",
        "cannot use `item` after it was moved",
    );
}

#[test]
fn memory_exchanges_reject_aliasing_and_checked_borrow_payloads() {
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { value := 1\nreference := &mut value\nmem.swap(reference, reference)\nreturn value as i32\n}\n",
        "overlapping borrows within the same expression",
    );
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { first := 1\nsecond := 2\nreference := &first\nold := mem.replace(&mut reference, &second)\nreturn *old as i32\n}\n",
        "replacing or swapping checked-borrow values is unsupported",
    );
}

#[test]
fn memory_exchanges_cannot_discard_new_result_obligations() {
    // Matching through a reference handles the old value without moving it.
    // Exchanging a new error into that binding must not silently discard it.
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { value: i32!u8 = ok(1)\nmatch &value { ok(_) => {}, err(_) => {} }\nold := mem.replace(&mut value, err(2u8))\nmatch old { ok(_) => {}, err(_) => {} }\nreturn 0\n}\n",
        "Result-containing values is unsupported",
    );
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { first: i32!u8 = ok(1)\nmatch &first { ok(_) => {}, err(_) => {} }\nsecond: i32!u8 = err(2)\nmem.swap(&mut first, &mut second)\nmatch second { ok(_) => {}, err(_) => {} }\nreturn 0\n}\n",
        "Result-containing values is unsupported",
    );
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { first: Option<i32!u8> = none\nsecond: Option<i32!u8> = some(err(2))\nmem.swap(&mut first, &mut second)\nreturn 0\n}\n",
        "Result-containing values is unsupported",
    );
    rejects(
        "package app\nimport \"core/mem\"\nstruct Outcome { result: i32!u8 }\nfn main() -> i32 { first := Outcome { result: ok(1) }\nsecond := Outcome { result: err(2) }\nmem.swap(&mut first, &mut second)\nreturn 0\n}\n",
        "Result-containing values is unsupported",
    );
}

#[test]
fn indirect_assignments_cannot_erase_or_publish_result_obligations() {
    // Matching the old binding cleared its sole pending_result bit; indirect
    // stores did not set it again or check an old nested pending obligation.
    for body in [
        "r: i32!u8 = ok(1)\nmatch &r { ok(_) => {}, err(_) => {} }\nalias := &mut r\n*alias = err(2)",
        "r := Outcome { result: ok(1) }\nmatch &r { Outcome { result: ok(_) } => {}, Outcome { result: err(_) } => {} }\nr.result = err(2)",
        // Overwriting a still-pending nested value is also forbidden.
        "r := Outcome { result: err(1) }\nr.result = ok(2)\nmatch r { Outcome { result: ok(_) } => {}, Outcome { result: err(_) } => {} }",
        "r: [1]Result<i32, u8> = [err(1)]\nr[0] = ok(2)",
        "r: Option<i32!u8> = none\nmatch &r { some(value) => { match value { ok(_) => {}, err(_) => {} } }, none => {} }\nalias := &mut r\n*alias = some(err(2))",
    ] {
        rejects(
            &format!("package app\nstruct Outcome {{ result: i32!u8 }}\nfn main() {{\n{body}\n}}"),
            "assigning Result-containing values through a field, index, or reference is unsupported",
        );
    }
    for body in [
        "fn store(out: &mut Result<i32, u8>) { *out = err(2) }",
        "fn store(out: &mut[Result<i32, u8>]) { out[0] = err(2) }",
        "fn store(out: &mut Option<Result<i32, u8>>) { *out = none }",
        "fn store(out: &mut Outcome) { out.result = err(2) }",
        "fn store(out: &mut[Outcome]) { out[0] = Outcome { result: err(2) } }",
        "fn store(out: &mut[Envelope]) { out[0] = Envelope.Error(err(2)) }",
        "fn store(out: &mut [1]Result<i32, u8>) { *out = [err(2)] }",
        "fn store(out: &mut Result<i32, u8>, value: Result<i32, u8>) { *out = value }",
        // A callee handling the old referent cannot publish a fresh obligation.
        "fn store(out: &mut Result<i32, u8>) { match &*out { ok(_) => {}, err(_) => {} }\n*out = err(2) }",
    ] {
        rejects(
            &format!(
                "package app\nstruct Outcome {{ result: i32!u8 }}\nenum Envelope {{ Error(i32!u8), Empty }}\n{body}\n"
            ),
            "assigning Result-containing values through a field, index, or reference is unsupported",
        );
    }
}

#[test]
fn whole_binding_replacement_keeps_new_results_pending_on_every_exit() {
    for tail in [
        "r = err(2)",
        "r = err(2)\nr = ok(3)",
        "if flag { r = err(2) }",
        "for flag { r = err(2)\nbreak }",
        "r = err(2)\ncore.drop(r)",
        "r = err(2)\n_ = r",
    ] {
        rejects(
            &format!(
                "package app\nfn test(flag: bool) {{ r: i32!u8 = ok(1)\nmatch &r {{ ok(_) => {{}}, err(_) => {{}} }}\n{tail}\n}}"
            ),
            "Result",
        );
    }
}

#[test]
fn raw_copy_requires_unsafe_and_a_writable_destination() {
    rejects(
        "package app\nimport \"core/ptr\"\nfn main() -> i32 { source := 1u8\ndestination := 0u8\nptr.copy(ptr.from_ref(&source), ptr.from_mut(&mut destination), 1usize)\nreturn 0\n}\n",
        "raw pointer operation requires an explicit unsafe block",
    );
    rejects(
        "package app\nimport \"core/ptr\"\nfn main() -> i32 { source := 1u8\ndestination := 0u8\nunsafe { ptr.copy_nonoverlapping(ptr.from_ref(&source), ptr.from_ref(&destination), 1usize) }\nreturn 0\n}\n",
        "expected `*mut u8`, found `*const u8`",
    );
}

#[test]
fn raw_checked_views_require_unsafe_valid_owner_and_plain_payloads() {
    rejects(
        "package app\nimport \"core/ptr\"\nfn main() -> i32 { value := 1u8\npointer := ptr.from_ref(&value)\nview := ptr.borrow(pointer, &value)\nreturn *view as i32\n}",
        "requires an explicit unsafe block",
    );
    rejects(
        "package app\nimport \"core/ptr\"\nfn main() -> i32 { value := 1u8\npointer := ptr.from_mut(&mut value)\nview := unsafe { ptr.borrow_mut(pointer, &value) }\nreturn *view as i32\n}",
        "exclusive owner borrow",
    );
    rejects(
        "package app\nimport \"core/ptr\"\nfn escape() -> &[u8] from(static) { value := 1u8\npointer := ptr.from_ref(&value)\nunsafe { return ptr.borrow_slice(pointer, 1, &value) }\n}",
        "borrow",
    );
    for ty in ["&u8", "Result<u8, u8>", "Option<u8!u8>"] {
        rejects(
            &format!(
                "package app\nimport \"core/ptr\"\nfn main() -> i32 {{ owner := 1u8\nunsafe {{ pointer := 1usize as *const {ty}\nview := ptr.borrow(pointer, &owner)\n}}\nreturn 0\n}}"
            ),
            "cannot reconstruct checked-borrow elements or unhandled Results",
        );
    }
    rejects(
        "package app\nimport \"core/mem\"\nfn escape() -> &str from(static) { bytes := [65u8]\nunsafe { return mem.str_from_utf8(&bytes) }\n}",
        "borrow",
    );
    rejects(
        "package app\nimport \"core/mem\"\nfn main() -> i32 { text := mem.str_from_utf8(b\"hello\")\nreturn text.len as i32\n}",
        "requires an explicit unsafe block",
    );
}

#[test]
fn allocated_views_prevent_growth_moving_destruction_and_aliasing() {
    let prefix = "package app\nimport \"alloc/arena\"\nimport \"std/arena_bytes\"\nfn main() -> i32 { backing := [0u8; 64]\nallocator := arena.Arena.new(&mut backing)\nmatch arena_bytes.new(&mut allocator, 4, 32) { ok(buffer) => {\nmatch buffer.extend(b\"abc\") { ok() => {}, err(_) => { return 2 } }\n";
    for invalid in [
        "match buffer.extend(buffer.as_slice()) { ok() => {}, err(_) => {} }\nreturn 0",
        "view := buffer.as_slice()\nmatch buffer.reserve(16) { ok() => {}, err(_) => {} }\nreturn view.len as i32",
        "view := buffer.as_slice()\ncore.drop(buffer)\nreturn view.len as i32",
        "view := buffer.as_slice()\nother := buffer\nreturn view.len as i32",
        "view := buffer.as_mut_slice()\nother := buffer.as_slice()\nview[0] = other[0]\nreturn 0",
        "view := buffer.as_slice()\nallocator := allocator\nreturn view.len as i32",
    ] {
        rejects(
            &format!("{prefix}{invalid}\n}}, err(_) => {{ return 1 }} }}\n}}"),
            "borrow",
        );
    }
    rejects(
        &format!("{prefix}buffer.reserve(16)\nreturn 0\n}}, err(_) => {{ return 1 }} }}\n}}"),
        "Result",
    );
}

#[test]
fn disjoint_adapter_fields_still_reject_aliasing_at_the_call_boundary() {
    rejects(
        "package app\nstruct Pair { a: &mut u8\nb: &mut u8 }\nfn use_pair(pair: &mut Pair) { *pair.a = 1\n*pair.b = 2 }\nfn main() -> i32 { value := 0u8\npair := Pair { a: &mut value, b: &mut value }\nuse_pair(&mut pair)\nreturn value as i32\n}",
        "borrow",
    );
    rejects(
        "package app\nstruct ReadOnly { value: &u8 }\nfn mutate(view: &mut ReadOnly) { *view.value = 2 }",
        "immutable",
    );
}

#[test]
fn borrowed_aggregate_arguments_retain_exclusive_reference_capabilities() {
    let pair = "fn mutate(a: &mut[u8], b: &mut[u8]) { a[0] = 1\nb[0] = 2 }\n";
    for body in [
        "struct Holder { data: &mut[u8] }\nfn bad(holders: &[Holder]) { mutate(holders[0].data, holders[0].data) }",
        "fn bad(holders: &[&mut[u8]]) { mutate(holders[0], holders[0]) }",
        "struct Holder { data: &mut[u8] }\nfn bad(holder: Holder) { mutate(holder.data, holder.data) }",
        "fn bad(holder: Option<&mut[u8]>) { match holder { some(data) => { mutate(data, data) }, none => {} } }",
        "struct Holder { data: &mut[u8] }\nfn bad(holder: &Holder) { mutate(holder.data, holder.data) }",
        "struct Holder { data: &mut[u8] }\nfn bad(holder: &Holder) { holder.data[0] = 1 }",
        "struct Holder { data: &mut[u8] }\nfn bad(holder: &Holder) { data := &mut holder.data[..]\ndata[0] = 1 }",
    ] {
        let source = Source::new(&format!("package app\n{pair}{body}\n"));
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source.0)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "invalid exclusive alias accepted: {body}"
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked at"));
    }
}
