//! Split-specific ownership checks and native execution at both optimization levels.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-split-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn source(&self, body: &str) -> PathBuf {
        let path = self.0.join("main.dodo");
        let source = if body.starts_with("package ") {
            body.to_owned()
        } else {
            format!("package split_tests\nimport \"core/slice\"\nimport \"core/mem\"\n{body}\n")
        };
        fs::write(&path, source).unwrap();
        path
    }
    fn run(&self, body: &str, stdout: &[u8]) {
        let source = self.source(body);
        for level in ["0", "3"] {
            let executable = self
                .0
                .join(format!("split-O{level}{}", std::env::consts::EXE_SUFFIX));
            let built = Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["-O", level, "-o"])
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                built.status.success(),
                "O{level}: {}",
                String::from_utf8_lossy(&built.stderr)
            );
            let ran = Command::new(executable).output().unwrap();
            assert!(
                ran.status.success(),
                "O{level}: {}\n{}",
                ran.status,
                String::from_utf8_lossy(&ran.stderr)
            );
            assert_eq!(ran.stdout, stdout, "O{level}");
        }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn rejects(body: &str, message: &str) {
    let workspace = Workspace::new();
    let source = workspace.source(body);
    for level in ["0", "3"] {
        let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(["--emit", "obj", "-O", level, "-o"])
            .arg(workspace.0.join("invalid.o"))
            .output()
            .unwrap();
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            !result.status.success(),
            "incorrectly accepted at O{level}: {body}"
        );
        assert!(
            error.contains(message),
            "expected {message:?}, got {error}\n{body}"
        );
        assert!(!error.contains("panicked at"), "{error}");
    }
}

#[test]
fn native_split_boundaries_nested_reborrows_and_returns() {
    Workspace::new().run(include_str!("stdlib/slice_split.dodo"), b"");
}

#[test]
fn split_keeps_ordinary_index_and_subslice_alias_checks() {
    for body in [
        "a := [1i32, 2i32]; x := &mut a[0]; y := &mut a[1]; *x = *y",
        "a := [1i32, 2i32]; x := &mut a[..1]; y := &mut a[1..]; x[0] = y[0]",
        "a := [1i32, 2i32]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut a, 1) else { return }; a[0] = right[0]; left[0] = 3",
        "a := [1i32, 2i32]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut a, 1) else { return }; x := &mut left[0]; y := &mut left[0]; *x = *y; right[0] = 3",
        "a := [1i32, 2i32]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut a, 1) else { return }; core.drop(a); left[0] = right[0]",
    ] {
        rejects(&format!("fn main() {{ {body} }}"), "borrow");
    }
    rejects(
        r#"
import "core/ptr"
fn main() {
    a := [1i32, 2i32]
    pointer := ptr.as_mut_ptr(&mut a)
    left := unsafe { ptr.borrow_slice_mut(pointer, 1, &mut a) }
    right := unsafe { ptr.borrow_slice_mut(ptr.offset(pointer, 1), 1, &mut a) }
    left[0] = right[0]
}
"#,
        "borrow",
    );
}

#[test]
fn split_rejects_escapes_moves_parent_reuse_and_forged_pairs() {
    let cases = [
        (
            "fn main() { a := [1i32, 2i32]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut a, 1) else { return }; core.drop(left); left[0] = right[0] }",
            "moved",
        ),
        (
            "fn corrupt(p: &mut slice.SplitMut<i32>, data: &mut[i32]) { p.left = data }",
            "SplitMut slice fields cannot be replaced",
        ),
        (
            "fn main() { a := [1i32, 2i32]; p := slice.split_at_mut(&mut a, 1); core.drop(p); core.drop(p) }",
            "moved",
        ),
        (
            "fn main() { a := [1i32, 2i32]; let some(p) = slice.split_at_mut(&mut a, 1) else { return }; let slice.SplitMut{left, right} = p; core.drop(p); left[0] = right[0] }",
            "moved",
        ),
        (
            "fn bad() -> &mut[i32] from(static) { a := [1i32, 2i32]; let some(slice.SplitMut{left, right: _}) = slice.split_at_mut(&mut a, 1) else { for { assert(false) } }; return left }",
            "cannot return a borrow",
        ),
        (
            "fn bad() -> Option<slice.SplitMut<i32>> from(static) { a := [1i32, 2i32]; return slice.split_at_mut(&mut a, 1) }",
            "cannot return a borrow",
        ),
        (
            "fn bad(a: &mut[i32], b: &mut[i32]) -> Option<slice.SplitMut<i32>> from(a) { return slice.split_at_mut(b, 1) }",
            "outside the return contract",
        ),
        (
            "fn main() { out: &mut[i32]; { a := [1i32, 2i32]; let some(slice.SplitMut{left, right: _}) = slice.split_at_mut(&mut a, 1) else { return }; out = left }; _ = out.len }",
            "outlives its source",
        ),
        (
            "fn main() { a := [1i32, 2i32]; parent := &mut a[..]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(parent, 1) else { return }; parent[0] = 3; left[0] = right[0] }",
            "borrow",
        ),
        (
            "fn main() { a := [1i32, 2i32, 3i32]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut a, 2) else { return }; let some(slice.SplitMut{left: nested, right: tail}) = slice.split_at_mut(left, 1) else { return }; left[0] = 3; nested[0] = tail[0] + right[0] }",
            "borrow",
        ),
        (
            "fn use(a: &mut[i32], b: &mut[i32]) {} fn main() { a := [1i32, 2i32]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut a, 1) else { return }; use(left, left); _ = right.len }",
            "borrow",
        ),
        (
            "fn main() { a := [1i32, 2i32]; let some(p) = slice.split_at_mut(&mut a, 1) else { return }; q := p; q.left = &mut q.right[..] }",
            "SplitMut slice fields cannot be replaced",
        ),
        (
            "fn main() { a := [1i32]; b := [2i32]; p := slice.SplitMut<i32>{left: &mut a, right: &mut b} }",
            "SplitMut must be constructed",
        ),
        (
            "fn main() { a := [1i32, 2i32]; let some(p) = slice.split_at_mut(&mut a, 1) else { return }; q := p; view := &mut q.left[..]; let slice.SplitMut{left, right} = q; view[0] = left[0] + right[0] }",
            "borrow",
        ),
        (
            "fn main() { a := [1i32, 2i32]; let some(p) = slice.split_at_mut(&mut a, 1) else { return }; let slice.SplitMut{left, right} = &p; left[0] = right[0] }",
            "shared",
        ),
        (
            "fn main() { a := [1i32, 2i32]; let some(p) = slice.split_at_mut(&mut a, 1) else { return }; let slice.SplitMut{left, right} = &mut p; x := &mut left[..]; y := &mut right[..]; x[0] = y[0] }",
            "immutable",
        ),
        (
            "fn main() { a := [1i32, 2i32]; out: &mut[i32]; for mid in 0usize..2usize { let some(slice.SplitMut{left, right: _}) = slice.split_at_mut(&mut a, mid) else { return }; out = left }; _ = out.len }",
            "split view cannot escape its loop iteration",
        ),
        (
            "fn main() { a := [1i32]; _ = slice.split_at_mut(&a, 0) }",
            "expected `&mut",
        ),
        (
            "fn main() { a := [1i32]; _ = slice.split_at_mut(&mut a, -1isize) }",
            "expected `usize`",
        ),
        (
            "fn main() { a := [1i32]; _ = mem.split_at_mut::<i32>(&mut a, 0) }",
            "requires slice.SplitMut<T>",
        ),
        (
            "fn main() { _ = mem.split_at_mut() }",
            "expects a SplitMut type argument",
        ),
        (
            "fn mutate(a: &mut[usize]) -> usize { a[0] = 0; return 0 } fn main() { a := [1usize]; _ = slice.split_at_mut(&mut a, mutate(&mut a)) }",
            "borrow",
        ),
    ];
    for (body, diagnostic) in cases {
        rejects(body, diagnostic);
    }
}

#[test]
fn split_zero_sized_elements_and_destruction() {
    Workspace::new().run(r#"
unsafe extern "C" fn putchar(n: i32) -> i32
struct Zero { fn drop(&mut self) { unsafe { putchar(90) } } }
struct Token { n: i32; fn drop(&mut self) { unsafe { putchar(self.n) } } }
struct Watch { view: &mut[i32]; fn drop(&mut self) { unsafe { putchar(self.view[0]) } } }
fn zst(a: &mut Zero, b: &mut Zero) {}
fn main() {
    zeros := [Zero{}, Zero{}, Zero{}]
    assert_eq(mem.size_of::<Zero>(), 0usize)
    for mid in 0usize..4usize {
        let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut zeros, mid) else { for { assert(false) } }
        assert_eq(left.len + right.len, 3usize)
    }
    let some(slice.SplitMut{left: zl, right: zr}) = slice.split_at_mut(&mut zeros, 1) else { for { assert(false) } }
    zst(&mut zl[0], &mut zr[0])
    core.drop(zl)
    core.drop(zr)
    core.drop(zeros)
    tokens := [Token{n: 65}, Token{n: 66}]
    parts := slice.split_at_mut(&mut tokens, 1)
    core.drop(parts)
    assert_eq(tokens[0].n, 65i32)
    core.drop(tokens)
    numbers := [67i32, 68i32]
    {
        let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut numbers, 1) else { for { assert(false) } }
        first := Watch{view: left}
        second := Watch{view: right}
    }
    numbers[0] = 69
    unsafe { putchar(numbers[0]) }
}
"#, b"ZZZBADCE");
    rejects(
        r#"
struct Watch { view: &mut[i32]; fn drop(&mut self) { self.view[0] += 1 } }
fn main() { a := [1i32, 2i32]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut a, 1) else { return }; watcher := Watch{view: left}; _ = right.len; core.drop(a) }
"#,
        "borrow",
    );
}

#[test]
fn split_preserves_shared_allocator_and_element_dependencies() {
    Workspace::new().run(r#"
import "alloc/shared_arena"
import "alloc/error"
import "std/collections/shared_vector"
fn check() -> void!error.AllocError {
    bytes := [0u8; 2048]
    arena := shared_arena.SharedArena.new(&mut bytes)?
    values := shared_vector.new::<i32>(arena.handle())
    other := shared_vector.new::<i32>(arena.handle())
    values.push(1)?
    values.push(2)?
    let some(slice.SplitMut{left, right}) = slice.split_at_mut(values.as_mut_slice(), 1) else { for { assert(false) } }
    other.push(99)?
    _ = arena.used()
    left[0] = 10
    right[0] = 20
    assert_eq(left[0] + right[0], 30i32)
    values.push(30)?
    assert_eq(values.as_slice()[2], 30i32)
    return ok()
}
fn main() {
    match check() { ok() => {}, err(_) => { for { assert(false) } } }
    x := 1i32
    y := 2i32
    refs := [&x, &y]
    let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut refs, 1) else { for { assert(false) } }
    assert_eq(*left[0] + *right[0], 3i32)
    x = 10
    y = 20
}
"#, b"");
    let prefix = r#"
import "alloc/shared_arena"
import "alloc/error"
import "std/collections/shared_vector"
fn main() -> void!error.AllocError {
    bytes := [0u8; 2048]
    arena := shared_arena.SharedArena.new(&mut bytes)?
    values := shared_vector.new::<i32>(arena.handle())
    values.push(1)?
    values.push(2)?
    let some(slice.SplitMut{left, right}) = slice.split_at_mut(values.as_mut_slice(), 1) else { for { assert(false) } }
"#;
    for access in [
        "core.drop(values)",
        "core.drop(arena)",
        "unsafe { arena.reset() }",
        "bytes[0] = 1",
        "values.push(3)?",
    ] {
        rejects(
            &format!("{prefix}\n{access}\nleft[0] = right[0]\nreturn ok()\n}}"),
            "borrow",
        );
    }
    rejects(
        "fn main() { x := 1i32; refs := [&x, &x]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut refs, 1) else { return }; x = 2; _ = *left[0] + *right[0] }",
        "borrow",
    );
}

#[test]
fn split_joins_keep_all_partitions_and_nested_sources() {
    // Splitting storage must not partition the exclusive lifetimes stored in it.
    rejects(
        "fn main() { x := 1i32; y := 2i32; refs := [&mut x, &mut y]; let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut refs, 1) else { return }; _ = *left[0] + *right[0] }",
        "borrow",
    );
    Workspace::new().run(
        r#"
fn choose(data: &mut[i32], flag: bool) -> Option<slice.SplitMut<i32>> from(data) {
    result: Option<slice.SplitMut<i32>>
    if flag { result = slice.split_at_mut(data, 1) } else { result = slice.split_at_mut(data, 2) }
    return result
}
struct Holder { pair: slice.SplitMut<i32> }
fn main() {
    a := [1i32, 2i32, 3i32]
    for i in 0usize..2usize {
        let some(p) = choose(&mut a, i == 0) else { for { assert(false) } }
        h := Holder{pair: p}
        let Holder{pair} = h
        let slice.SplitMut{left, right} = pair
        assert_eq(left.len, i + 1)
        left[0] += 10
        right[0] += 20
    }
    assert_eq(a[0], 21i32)
    assert_eq(a[1], 22i32)
    assert_eq(a[2], 23i32)
}
"#,
        b"",
    );
    rejects(
        r#"
fn both(a: &mut[i32], b: &mut[i32]) {}
fn main() {
    a := [1i32, 2i32]
    let some(slice.SplitMut{left, right}) = slice.split_at_mut(&mut a, 1) else { return }
    x: &mut[i32]
    y: &mut[i32]
    if true { x = left; y = right } else { x = right; y = left }
    both(x, y)
}
"#,
        "borrow",
    );
    rejects(
        "fn main() { a := [1i32, 2i32]; let some(p) = slice.split_at_mut(&mut a, 1) else { return }; q := p; let slice.SplitMut{left, right} = &mut q; x := &mut left[..]; y := &mut right[..]; x[0] = y[0] }",
        "borrow",
    );
}

#[test]
fn direct_split_intrinsic_checks_bounds_before_pointer_arithmetic() {
    let workspace = Workspace::new();
    let source = workspace.source(
        "fn main() { a := [1i32]; _ = mem.split_at_mut::<slice.SplitMut<i32>>(&mut a, 2) }",
    );
    for level in ["0", "3"] {
        let executable = workspace.0.join(format!("trap-O{level}"));
        let built = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(["-O", level, "-o"])
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let result = Command::new(&executable).output().unwrap();
        assert!(
            !result.status.success(),
            "out-of-bounds intrinsic succeeded at O{level}"
        );
    }
}
