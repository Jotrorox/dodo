//! Core foundations: native execution, ownership, and unsafe boundaries.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-core-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn source(&self, source: &str) -> PathBuf {
        let path = self.0.join("main.dodo");
        fs::write(&path, source).unwrap();
        path
    }
    fn run(&self, source: &str, expected: &[u8]) {
        let input = self.source(source);
        for level in [0, 3] {
            let output = self.0.join(format!("core-O{level}"));
            let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["build"])
                .arg(&input)
                .arg("-o")
                .arg(&output)
                .args(["-O", &level.to_string()])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "O{level} compile failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let result = Command::new(output).output().unwrap();
            assert!(
                result.status.success(),
                "O{level}: {}\n{}",
                result.status,
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(result.stdout, expected, "O{level} drop order");
        }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn rejects(body: &str, expected: &str) {
    let workspace = Workspace::new();
    let input = workspace.source(&format!(
        "package invalid\nimport \"core/mem\"\nimport \"core/ptr\"\n{body}\n"
    ));
    let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("check")
        .arg(input)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success(), "incorrectly accepted: {body}");
    assert!(
        stderr.contains(expected),
        "expected {expected:?}, got {stderr}"
    );
    assert!(
        !stderr.contains("panicked at"),
        "compiler panicked: {stderr}"
    );
}
#[test]
fn core_storage_pointers_and_owned_exchange() {
    Workspace::new().run(include_str!("fixtures/core_foundation.dodo"), b"ACBDE");
}
#[test]
fn opaque_storage_moves_without_dropping_contents() {
    Workspace::new().run(
        r#"package storage
import "core/mem"
unsafe extern "C" fn putchar(value: i32) -> i32
struct Token { i32 id
 fn drop(self: &mut Self) { unsafe { putchar(self.id) } }
}
struct Wrapped<T> { MaybeUninit<T> storage }
fn forward<T>(value: MaybeUninit<T>) -> MaybeUninit<T> { return value }
fn main() {
 wrapped := Wrapped<Token>{storage: forward(mem.init(Token{id: 65}))}
 storage := mem.replace(&mut wrapped.storage, mem.uninit<Token>())
 value := unsafe { mem.assume_init(storage) }
 core.drop(value)
 ignored := Wrapped<Token>{storage: mem.uninit<Token>()}
 core.drop(ignored)
}
"#,
        b"A",
    );
}
#[test]
fn replacement_failure_preserves_original_and_drops_it_once() {
    Workspace::new().run(
        r#"package exchange
import "core/mem"
unsafe extern "C" fn putchar(value: i32) -> i32
struct Token { i32 id
 fn drop(self: &mut Self) { unsafe { putchar(self.id) } }
}
fn fail() -> Token!i32 { return err(1) }
fn attempt() -> void!i32 {
 value := Token{id: 65}
 old := mem.replace(&mut value, fail()?)
 core.drop(old)
 return ok()
}
fn main() { match attempt() { ok() => {} err(_) => {} } }
"#,
        b"A",
    );
}
#[test]
fn pointer_and_storage_errors_are_diagnostics() {
    let cases = [
        ("fn f() { ptr.copy() }", "expects 3 arguments"),
        ("fn f() { ptr.is_null() }", "expects 1 arguments"),
        ("fn f() { mem.uninit() }", "one type argument"),
        ("fn f() { mem.uninit<i32>(1) }", "no value arguments"),
        (
            "fn f() { mem.init<i32, u32>(1) }",
            "at most one type argument",
        ),
        ("fn f() { mem.init<i32>(1u32) }", "expected `i32`"),
        (
            "fn f() { value := mem.uninit<i32>(); mem.assume_init(value) }",
            "requires an explicit unsafe",
        ),
        (
            "fn f() { unsafe { mem.assume_init(1i32) } }",
            "requires MaybeUninit",
        ),
        (
            "fn f() { unsafe { mem.assume_init<i32, u32>(1i32) } }",
            "at most one type argument",
        ),
        (
            "fn f() { value := 1i32; ptr.from_mut(&value) }",
            "mutable raw pointer requires",
        ),
        (
            "fn f() { value := 1i32; ptr.as_ptr(&value) }",
            "requires a slice",
        ),
        (
            "fn f() { value := [1]u8{1u8}; ptr.as_mut_ptr(&value) }",
            "mutable raw pointer requires",
        ),
        (
            "fn f() { value := 1i32; ptr.is_null<u8>(ptr.from_ref(&value)) }",
            "does not match",
        ),
        (
            "fn f() { value := 1i32; ptr.read(ptr.from_ref(&value)) }",
            "requires an explicit unsafe",
        ),
        (
            "fn f() { value := 1i32; unsafe { ptr.write(ptr.from_ref(&value), 2i32) } }",
            "const raw pointer",
        ),
        (
            "fn f() { value := 1i32; unsafe { ptr.copy(ptr.from_ref(&value), ptr.from_ref(&value), 1usize) } }",
            "expected `*mut i32`",
        ),
        (
            "fn f() { value := 1i32; unsafe { ptr.write_bytes(ptr.from_mut(&mut value), 1u32, 1usize) } }",
            "expected `u8`",
        ),
        (
            "fn f() { value := 1i32; unsafe { ptr.drop_in_place(ptr.from_ref(&value)) } }",
            "mutable raw pointer",
        ),
        (
            "fn f() { value := 1i32; mem.replace(&value, 2i32) }",
            "requires a mutable reference",
        ),
        (
            "fn f() { value := 1i32; mem.swap(&mut value, &mut value) }",
            "borrow",
        ),
        (
            "fn f() { value := 1i32; reference := &mut value; mem.swap(reference, reference) }",
            "borrow",
        ),
        (
            "fn f() { value := 1i32; storage := mem.init(&value) }",
            "cannot hide checked borrows",
        ),
        (
            "fn f() { result: i32!i32 = ok(1i32); storage := mem.init(result) }",
            "unhandled Results",
        ),
        (
            "fn f() { storage := mem.uninit<&i32>(); unsafe { mem.assume_init(storage) } }",
            "checked-borrow",
        ),
        (
            "fn f() { value := 1i32; reference := &value; unsafe { ptr.read(ptr.from_ref(&reference)) } }",
            "checked-borrow",
        ),
        (
            "fn f() { value := 1i32; other := 2i32; a := &value; b := &other; mem.swap(&mut a, &mut b) }",
            "checked-borrow",
        ),
        (
            "fn f() { storage := mem.uninit<i32>(); pointer := mem.uninit_as_mut_ptr(&storage) }",
            "mutable reference",
        ),
        (
            "fn f() { storage := mem.uninit<i32>(); mem.uninit_as_ptr<u8>(&storage) }",
            "expected `u8`",
        ),
        (
            "fn f() { storage := mem.uninit<i32>(); core.drop(storage); core.drop(storage) }",
            "moved",
        ),
        (
            "fn f() { value: MaybeUninit<i32, u32> }",
            "one type argument",
        ),
        ("fn f() { value: MaybeUninit<void> }", "void is only valid"),
    ];
    for (body, expected) in cases {
        rejects(body, expected);
    }
}
#[test]
fn inline_slice_and_nested_reference_arguments_do_not_conflict_with_themselves() {
    Workspace::new().run(
        r#"package reborrows
import "core/mem"
fn fill(bytes: &mut [u8]) { bytes[0] = 9u8 }
fn forward(bytes: &mut [u8]) -> &mut [u8] from(bytes) { return bytes }
fn scalar(value: &mut i32) -> &mut i32 from(value) { return value }
fn main() -> i32 {
 values := [2]u8{1u8, 2u8}
 fill(&mut values[1..])
 if values[1] != 9u8 { return 1 }
 fill(forward(&mut values[..1]))
 if values[0] != 9u8 { return 2 }
 a := 1i32
 b := 2i32
 mem.swap(scalar(&mut a), scalar(&mut b))
 if a != 2i32 || b != 1i32 { return 3 }
 return 0
}
"#,
        b"",
    );
    rejects(
        "fn use(a: &mut [u8], b: &[u8]) {} fn f() { values := [2]u8{1u8, 2u8}; use(&mut values[..], &values[..]) }",
        "borrow",
    );
    rejects(
        "fn forward(value: &mut i32) -> &mut i32 from(value) { return value } fn f() { value := 1i32; mem.swap(forward(&mut value), forward(&mut value)) }",
        "borrow",
    );
}
#[test]
fn public_generics_accept_private_arguments_without_exposing_private_declarations() {
    Workspace::new().run(
        r#"package generic_visibility
pub struct Container<T> {
 pub T value
 pub fn get(self: &Self) -> &T from(self) { return &self.value }
}
pub fn identity<T>(value: T) -> T { return value }
struct Private { i32 number }
fn main() -> i32 {
 container := Container<Private>{value: identity(Private{number: 17})}
 if container.get().number != 17i32 { return 1 }
 return 0
}
"#,
        b"",
    );
    rejects(
        "struct Private {} pub fn leak<T>(value: Private) {}",
        "public API exposes private type",
    );
    rejects(
        "struct Private {} pub struct Container<T> { pub Private value }",
        "public API exposes private type",
    );
    rejects(
        "struct Private {} pub enum Container<T> { value(Private) }",
        "public API exposes private type",
    );
    rejects(
        "struct Private {} pub struct Container<T> { T value } pub fn leak(value: Container<Private>) {}",
        "public API exposes private type",
    );
}
#[test]
fn field_offsets_enforce_literal_direct_fields_and_package_privacy() {
    let cases = [
        ("fn f() { mem.offset_of() }", "one struct type argument"),
        (
            "struct S { value: u8 } fn f() { mem.offset_of::<S>() }",
            "one literal field name",
        ),
        (
            "struct S { value: u8 } fn f() { mem.offset_of::<S, S>(\"value\") }",
            "one struct type argument",
        ),
        (
            "struct S { value: u8 } fn f() { mem.offset_of::<S>(\"value\", \"value\") }",
            "one literal field name",
        ),
        (
            "fn f() { mem.offset_of::<u32>(\"value\") }",
            "requires a struct type",
        ),
        (
            "enum E { a, b } fn f() { mem.offset_of::<E>(\"a\") }",
            "requires a struct type",
        ),
        (
            "struct S { value: u8 } fn f() { mem.offset_of::<&S>(\"value\") }",
            "requires a struct type",
        ),
        (
            "struct S { value: u8 } fn f() { mem.offset_of::<S>(\"missing\") }",
            "unknown field",
        ),
        (
            "struct S { value: u8 } fn f() { mem.offset_of::<S>(\"value.inner\") }",
            "unknown field",
        ),
        (
            "struct S { value: u8 } fn f() { field := \"value\"; mem.offset_of::<S>(field) }",
            "requires a string literal",
        ),
        (
            "struct S { value: u8 } fn f() { mem.offset_of::<S>(b\"value\") }",
            "requires a string literal",
        ),
        (
            "struct S { value: u8 } fn f() { mem.offset_of::<S>(1u8) }",
            "requires a string literal",
        ),
    ];
    for (body, expected) in cases {
        rejects(body, expected);
    }
    let workspace = Workspace::new();
    fs::write(
        workspace.0.join("fields.dodo"),
        r#"package fields
import "core/mem"
@repr(C)
pub struct Public { pub prefix: u8, secret: u32, pub exposed: u8 }
pub fn own_offset() -> usize { return mem.offset_of::<Public>("secret") }
"#,
    )
    .unwrap();
    workspace.run(
        r#"package visible
import "core/mem"
import "fields"
fn main() -> i32 {
 if mem.offset_of::<fields.Public>("exposed") != 8usize { return 1 }
 if fields.own_offset() != 4usize { return 2 }
 return 0
}
"#,
        b"",
    );
    let input = workspace.source("package hidden\nimport \"core/mem\"\nimport \"fields\"\nfn main() -> usize { return mem.offset_of::<fields.Public>(\"secret\") }\n");
    let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("check")
        .arg(input)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("field `secret` is private to its package")
    );
}
