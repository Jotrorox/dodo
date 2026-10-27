//! Tuples and type aliases: native execution at both optimization levels, the
//! ownership and borrow rules tuples share with structs, and their diagnostics.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-tuples-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
    fn source(&self, body: &str) -> PathBuf {
        self.file("main.dodo", &format!("package main\n{body}\n"))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_in(workspace: &Workspace, source: &PathBuf) {
    for level in ["0", "3"] {
        let executable = workspace
            .0
            .join(format!("program-O{level}{}", std::env::consts::EXE_SUFFIX));
        let built = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
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
    }
}

fn run(body: &str) {
    let workspace = Workspace::new();
    let source = workspace.source(body);
    run_in(&workspace, &source);
}

fn check(workspace: &Workspace, source: &PathBuf) -> (bool, String) {
    let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("check")
        .arg(source)
        .output()
        .unwrap();
    let output = String::from_utf8_lossy(&result.stderr).into_owned()
        + &String::from_utf8_lossy(&result.stdout);
    (
        result.status.success(),
        output.replace(workspace.0.to_str().unwrap(), ""),
    )
}

fn rejects(body: &str, messages: &[&str]) {
    let workspace = Workspace::new();
    let source = workspace.source(body);
    let (success, output) = check(&workspace, &source);
    assert!(!success, "incorrectly accepted: {body}");
    for message in messages {
        assert!(
            output.contains(message),
            "missing `{message}` in:\n{output}\nfor:\n{body}"
        );
    }
    // Internal tuple and specialization names never reach the user.
    assert!(!output.contains('$') && !output.contains('#'), "{output}");
}

#[test]
fn tuples_construct_return_index_and_destructure() {
    run(r#"
const ORIGIN: (i32, (u8, bool)) = (0, (7, true))

fn divmod(a: i32, b: i32) -> (i32, i32) {
    (a / b, a % b)
}

fn swap<A, B>(pair: (A, B)) -> (B, A) {
    let (a, b) = pair
    return (b, a)
}

fn split(bytes: &[u8], at: usize) -> (&[u8], &[u8]) from(bytes) {
    (&bytes[..at], &bytes[at..])
}

fn main() {
    assert_eq(ORIGIN.1.0, 7u8)
    assert(ORIGIN.1.1)
    let (q, r) = divmod(17, 5)
    assert_eq(q, 3)
    assert_eq(r, 2)
    pair := divmod(9, 4)
    assert_eq(pair.0 * 4 + pair.1, 9)

    swapped := swap((7u8, true))
    assert(swapped.0)
    assert_eq(swapped.1, 7u8)

    let (head, tail) = split(b"tuples", 3)
    assert_eq(head.len, 3)
    assert_eq(tail.len, 3)

    nested := ((1, 2), (3, (4u16, 5i64)))
    assert_eq(nested.1.1.0, 4u16)
    assert_eq(nested.1.1.1, 5i64)
    let ((a, _), (_, (b, c))) = nested
    assert_eq(a + 2, 3)
    assert_eq(b, 4u16)
    assert_eq(c, 5i64)

    explicit: (u8, (bool, f64)) = (255, (false, 1.5))
    assert_eq(explicit.0, 255u8)
    assert(!explicit.1.0)
    assert(explicit.1.1 > 1.0)

    counter := (0u32, 10u32)
    counter.0 += 1
    counter.1 = counter.0 * 5
    assert_eq(counter.0, 1u32)
    assert_eq(counter.1, 5u32)
}
"#);
}

#[test]
fn tuple_patterns_match_and_loops_destructure_elements() {
    run(r#"
fn classify(pair: (bool, i32)) -> i32 {
    match pair {
        (true, 0) => 1,
        (true, n) if n > 0 => 2,
        (true, _) => 3,
        (false, 0 | 1) => 4,
        (false, _) => 5,
    }
}

fn main() {
    assert_eq(classify((true, 0)), 1)
    assert_eq(classify((true, 9)), 2)
    assert_eq(classify((true, -9)), 3)
    assert_eq(classify((false, 1)), 4)
    assert_eq(classify((false, 7)), 5)

    found: Option<(u8, &str)> = some((2, "two"))
    match &found {
        some((n, _)) => assert_eq(*n, 2u8),
        none => assert(false),
    }
    if let some((2, name)) = found {
        assert_eq(name.len, 3)
    } else {
        assert(false)
    }

    scores := [(1u32, 10u32), (2, 20), (3, 30)]
    sum := 0u32
    for (id, score) in scores {
        sum += *id * *score
    }
    assert_eq(sum, 140u32)
    copied := 0u32
    for &(id, score) in scores {
        copied += id + score
    }
    assert_eq(copied, 66u32)
    weighted := 0usize
    for i, (_, score) in scores {
        weighted += i * (*score as usize)
    }
    assert_eq(weighted, 80usize)
    for (id, score) in &mut scores {
        *score += *id
    }
    assert_eq(scores[2].1, 33u32)

    grid := [[(0u8, 0u8), (0, 0)], [(0, 0), (0, 0)]]
    grid[1][0] = (1, 2)
    assert_eq(grid[1][0].1, 2u8)
}
"#);
}

#[test]
fn tuples_own_and_drop_their_elements_and_borrow_fields_disjointly() {
    run(r#"
struct Guard {
    count: &mut u32

    pub fn drop(&mut self) {
        *self.count += 1
    }
}

fn keep(pair: (Guard, u8)) -> u8 {
    pair.1
}

fn main() {
    drops := 0u32
    {
        pair := (Guard { count: &mut drops }, 5u8)
        assert_eq(keep(pair), 5u8)
    }
    assert_eq(drops, 1u32)

    values := (1, 2)
    first := &mut values.0
    second := &mut values.1
    *first += 10
    *second += 20
    assert_eq(values.0 + values.1, 33)
}
"#);
}

#[test]
fn type_aliases_name_types_structs_enums_and_generic_results() {
    let workspace = Workspace::new();
    workspace.file(
        "geo/geo.dodo",
        r#"package geo

pub struct Vec2 {
    pub x: i32
    pub y: i32

    pub fn new(x: i32, y: i32) -> Vec2 {
        Vec2 { x: x, y: y }
    }
}

pub enum Shape {
    Dot,
    Square(i32),
}

pub struct Cell<T> {
    pub value: T

    pub fn wrap(value: T) -> Cell<T> {
        Cell { value: value }
    }
}

pub type Point = Vec2
pub type Kind = Shape
pub type Ints = Cell<i32>
pub type Res<T> = T!Shape
pub type Span = (Point, Point)
type Width = u16

pub fn width() -> Width {
    3
}

pub fn checked(n: i32) -> Res<i32> {
    if n < 0 {
        return err(Kind.Dot)
    }
    ok(n)
}
"#,
    );
    let source = workspace.source(
        r#"import "geo"

type Count = u32
type Pair<T> = (T, T)
type Grid<T> = [2][2]T
type Local = geo.Point

fn area(span: &geo.Span) -> i32 {
    (span.1.x - span.0.x) * (span.1.y - span.0.y)
}

fn total(values: Pair<Count>) -> Count {
    values.0 + values.1
}

fn main() {
    n: Count = 4
    assert_eq(total((n, 6)), 10u32)
    g: Grid<u8> = [[1, 2], [3, 4]]
    assert_eq(g[1][0], 3u8)

    a := Local { x: 1, y: 2 }
    b := Local.new(4, 6)
    let Local { x, y } = b
    assert_eq(x + y, 10)
    span: geo.Span = (a, geo.Point.new(5, 7))
    assert_eq(area(&span), 20)

    kind := geo.Kind.Square(3)
    match kind {
        geo.Kind.Square(side) => assert_eq(side, 3),
        geo.Kind.Dot => assert(false),
    }
    cell := geo.Ints.wrap(8)
    assert_eq(cell.value, 8)

    assert_eq(geo.checked(5)!, 5)
    match geo.checked(-1) {
        ok(_) => assert(false),
        err(geo.Shape.Dot) => {}
        err(_) => assert(false),
    }
    assert_eq(geo.width(), 3u16)
}
"#,
    );
    run_in(&workspace, &source);
}

#[test]
fn tuples_and_aliases_combine_with_function_values_and_closures() {
    run(r#"
import "std/collections"

type Compare = fn(i32, i32) -> bool
type Step<T> = &mut dyn fn(T)

fn less(a: i32, b: i32) -> bool {
    a < b
}

fn sum_pairs(values: &[(i32, i32)], step: Step<i32>) {
    for &(a, b) in values {
        step(a + b)
    }
}

fn main() {
    compare: Compare = less
    assert(compare(1, 2))
    both := (less, fn(a: i32, b: i32) -> bool { a > b })
    greater := both.1
    assert(greater(3, 2))
    assert((both.0)(1, 2))

    pairs := [(1i32, 2i32), (3, 4)]
    total := 0i32
    sum_pairs(&pairs, fn(x) { total += x })
    assert_eq(total, 10)

    keyed := [(3i32, 30u8), (1, 10), (2, 20)]
    collections.sort_by(&mut keyed, fn(x, y) { x.0 - y.0 })
    assert_eq(keyed[0].1, 10u8)

    make := fn(n: i32) { (n, n * 2) }
    made := make(4)
    assert_eq(made.1, 8)
}
"#);
}

#[test]
fn tuple_and_alias_mistakes_have_source_level_diagnostics() {
    rejects(
        "fn main() { x: (i32, bool) = (1, 2) }",
        &["expected `bool`, found `isize`"],
    );
    rejects(
        "fn main() { x: (i32, bool) = (1, true, 3) }",
        &["expected `(i32, bool)`, found `(isize, bool, isize)`"],
    );
    rejects(
        "fn main() {\nx := (1, true)\ny := x.2\n}",
        &["type `(isize, bool)` has no field `2`"],
    );
    rejects(
        "fn main() { x := (none, 1) }",
        &["cannot infer the type of tuple element 0"],
    );
    rejects(
        "fn main() {\nx := ([1, 2], 3)\ny := x\nz := x\n}",
        &["`x` is uninitialized or has been moved"],
    );
    rejects(
        "fn main() {\nx := (true, false)\nmatch x {\n(true, _) => {}\n(false, true) => {}\n}\n}",
        &["non-exhaustive match"],
    );
    rejects(
        "fn main() { a := (1, 2) == (1, 2) }",
        &["operator is not defined for `(isize, isize)`"],
    );
    rejects(
        "fn f(n: &i32) -> (&i32, i32) { return (n, 1) }\nfn g() -> (&i32, i32) {\nn := 1i32\nreturn f(&n)\n}\nfn main() {}",
        &["borrowed"],
    );
    rejects(
        "fn main() { x := () }",
        &["tuples need at least two elements"],
    );
    rejects(
        "fn main() { x := (1,) }",
        &["tuples need at least two elements"],
    );
    rejects(
        "fn main() { x: (i32,) = 1 }",
        &["tuples need at least two elements"],
    );
    rejects(
        "type A = B\ntype B = A\nfn main() {}",
        &["type alias `A` refers to itself"],
    );
    rejects(
        "type A<T> = [2]T\nfn main() { x: A = [1, 2] }",
        &["type alias `A` expects 1 type argument, found 0"],
    );
    rejects(
        "type A = i32\nfn main() { x: A<u8> = 1 }",
        &["type alias `A` has no type parameters"],
    );
    rejects(
        "type A = i32\nstruct A {}\nfn main() {}",
        &["duplicate declaration `A`"],
    );
    rejects(
        "struct S {}\npub type T = S\nfn main() {}",
        &["public API exposes private type `S`"],
    );
    rejects(
        "type P = (i32, i32)\nfn main() { x := P { a: 1 } }",
        &["type alias `P` names `(i32, i32)`, which is not a struct"],
    );
    rejects(
        "type A<T: Eq> = T\nfn main() {}",
        &["type alias parameters cannot have interface bounds"],
    );
    rejects(
        "struct Pair<A, B> { first: A\nsecond: B }\nfn take(p: Pair<i32, i32>) {}\nfn main() {\np := Pair { first: 1i32, second: true }\ntake(p)\n}",
        &["Pair<i32, i32>", "Pair<i32, bool>"],
    );
}

#[test]
fn private_aliases_stay_in_their_package() {
    let workspace = Workspace::new();
    workspace.file(
        "lib/lib.dodo",
        "package lib\ntype Hidden = i32\npub type Shown = i32\n",
    );
    let source =
        workspace.source("import \"lib\"\nfn main() {\na: lib.Shown = 1\nb: lib.Hidden = 2\n}");
    let (success, output) = check(&workspace, &source);
    assert!(!success);
    assert!(
        output.contains("type alias `lib.Hidden` is private to its package"),
        "{output}"
    );
}
