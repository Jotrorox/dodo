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
