//! Function values and closures: native execution at both optimization
//! levels, and the borrow and type diagnostics that keep them sound.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-closures-{}-{}",
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
        if body.starts_with("package ") {
            self.file("main.dodo", body)
        } else {
            self.file("main.dodo", &format!("package main\n{body}\n"))
        }
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
}

#[test]
fn function_pointers_and_closures_run() {
    run(r#"
fn double(x: i32) -> i32 {
    x * 2
}

fn apply(f: &dyn fn(i32) -> i32, x: i32) -> i32 {
    f(x)
}

fn each(values: &[i32], visit: &mut dyn fn(i32)) {
    for &value in values {
        visit(value)
    }
}

fn pick(negative: bool) -> fn(i32) -> i32 {
    if negative {
        fn(x) { -x }
    } else {
        double
    }
}

struct Button {
    on_press: fn(u32) -> u32
}

struct Listener {
    notify: &dyn fn(i32) -> i32
}

fn main() {
    offset: i32 = 10
    add := fn(x: i32) -> i32 { x + offset }
    assert_eq(apply(add, 1), 11)
    assert_eq(apply(double, 2), 4)
    total: i32 = 0
    values: [3]i32 = [1, 2, 3]
    each(&values, fn(v) { total += v })
    assert_eq(total, 6)
    assert_eq(pick(true)(5), -5)
    assert_eq(pick(false)(5), 10)
    button := Button { on_press: fn(n) { n + 1 } }
    assert_eq(button.on_press(41u32), 42u32)
    base: i32 = 1000
    listener := Listener { notify: fn(v) { v + base } }
    assert_eq(listener.notify(1), 1001)
    inferred := fn(a: i32, b: i32) { a * b }
    assert_eq(inferred(6, 7), 42)
}
"#);
}

#[test]
fn generic_inference_methods_and_nested_captures() {
    run(r#"
import "std/collections"

fn map<T, U>(input: &[T], output: &mut [U], f: &dyn fn(&T) -> U) {
    for i in 0..input.len {
        output[i] = f(&input[i])
    }
}

fn identity<T>(value: T) -> T {
    value
}

fn apply_twice<T>(f: fn(T) -> T, value: T) -> T {
    f(f(value))
}

fn double(x: i32) -> i32 { x * 2 }

struct Point {
    x: i32
    y: i32

    fn sum(&self) -> i32 {
        self.x + self.y
    }

    fn scaled(&self, factor: i32) -> i32 {
        times := fn(v: i32) -> i32 { v * factor + self.x - self.x }
        times(self.sum())
    }
}

struct Counter {
    count: i32

    fn bump(&mut self) {
        self.count += 1
    }
}

fn main() {
    input: [4]i32 = [3, 1, 4, 1]
    output: [4]i64 = [0; 4]
    map(&input, &mut output, fn(v) { *v as i64 * 10 })
    assert(output[2] == 40)

    descending := true
    values: [5]i32 = [5, 2, 9, 1, 7]
    collections.sort_by(&mut values, fn(a, b) { if descending { *b - *a } else { *a - *b } })
    assert(values[0] == 9 && values[4] == 1)

    same: fn(i32) -> i32 = identity
    assert_eq(same(7), 7)
    assert_eq(apply_twice(double, 3), 12)
    assert_eq(apply_twice(fn(s: u8) -> u8 { s + 1 }, 1u8), 3u8)
    sum := Point.sum
    point := Point { x: 2, y: 3 }
    assert_eq(sum(&point), 5)
    assert_eq(point.scaled(10), 50)

    counter := Counter { count: 0 }
    bump_twice := fn() {
        counter.bump()
        counter.bump()
    }
    bump_twice()
    bump_twice()
    assert_eq(counter.count, 4)

    total: i32 = 0
    outer := fn(n: i32) {
        inner := fn() { total += n }
        inner()
        inner()
    }
    outer(5)
    assert_eq(total, 10)

    calls: i32 = 0
    step := fn() { calls += 1 }
    twice := fn() {
        step()
        step()
    }
    twice()
    assert_eq(calls, 2)

    acc: i32 = 0
    for i in 0..4 {
        add := fn() { acc += i as i32 }
        add()
    }
    assert_eq(acc, 6)
}
"#);
}

#[test]
fn constant_tables_static_handlers_and_large_values() {
    run(r#"
import "std/embedded/hal"

const OPERATIONS: [3]fn(i32) -> i32 = [double, negate, square]
const STEPS: [2]fn(i32) -> i32 = [fn(x) { x + 1 }, fn(x) { x * 3 }]
static mut HOOK: fn() -> i32 = fn() { 7 }

fn double(x: i32) -> i32 { x * 2 }
fn negate(x: i32) -> i32 { -x }
fn square(x: i32) -> i32 { x * x }

static mut HANDLER: Option<fn()> = none
static mut TIMER: hal.Handler = hal.NO_HANDLER
static mut TICKS: u32 = 0

fn tick() {
    unsafe {
        TICKS += 1
    }
}

fn interrupt() {
    unsafe {
        if let some(handler) = &HANDLER {
            handler()
        }
    }
}

struct Big {
    data: [300]i32
}

fn first_big(value: Big) -> Big {
    value
}

fn main() {
    result: i32 = 3
    for operation in &OPERATIONS {
        result = operation(result)
    }
    assert_eq(result, 36)
    for step in &STEPS {
        result = step(result)
    }
    assert_eq(result, 111)
    assert_eq(unsafe { HOOK() }, 7)
    unsafe {
        HOOK = fn() { 8 }
    }
    assert_eq(unsafe { HOOK() } + STEPS[0](0), 9)
    unsafe {
        HANDLER = some(tick)
    }
    interrupt()
    interrupt()
    assert(unsafe { TICKS } == 2u32)
    assert(!unsafe { TIMER.run() })
    section := hal.Critical.enter()
    unsafe {
        TIMER.set(tick)
    }
    core.drop(section)
    assert(unsafe { TIMER.run() })
    unsafe {
        TIMER.clear()
    }
    assert(!unsafe { TIMER.is_set() })
    assert(unsafe { TICKS } == 3u32)

    big := Big { data: [7; 300] }
    pass: fn(Big) -> Big = first_big
    copied := pass(big)
    assert_eq(copied.data[299], 7)
    keep := fn(value: Big) -> Big { value }
    again := keep(copied)
    shared: &dyn fn(Big) -> Big = first_big
    last := shared(again)
    assert_eq(last.data[0], 7)
}
"#);
}

#[test]
fn closures_return_results_and_borrows_from_parameters() {
    run(r#"
enum Failure { OutOfRange }

fn parse_all(values: &[i32], check: &dyn fn(i32) -> i32!Failure) -> i32!Failure {
    total: i32 = 0
    for &value in values {
        total += check(value)?
    }
    return ok(total)
}

fn largest(values: &[i32], pick: &dyn fn(&[i32]) -> &i32) -> i32 {
    *pick(values)
}

fn main() {
    input: [4]i32 = [3, 1, 4, 1]
    limit: i32 = 100
    checked := parse_all(&input, fn(v) {
        if v < 0 || v > limit {
            return err(Failure.OutOfRange)
        }
        ok(v)
    })
    assert_eq(checked!, 9)
    strict: i32 = 3
    match parse_all(&input, fn(v) -> i32!Failure {
        if v > strict {
            return err(Failure.OutOfRange)
        }
        return ok(v)
    }) {
        ok(_) => { assert(false) },
        err(_) => {},
    }
    assert_eq(largest(&input, fn(items) { &items[2] }), 4)
}
"#);
}

#[test]
fn collection_algorithms_take_closures() {
    run(r#"
import "std/collections"

struct Person {
    id: u32
    age: u32
}

fn by_age(a: &Person, b: &Person) -> i32 {
    a.age as i32 - b.age as i32
}

fn main() {
    values := [30i32, 10, 20, 20, 5]
    calls := 0
    collections.sort_by(&mut values, fn(a, b) {
        calls += 1
        *a - *b
    })
    assert(calls > 0)
    assert(collections.is_sorted_by(&values, fn(a, b) { *a - *b }))
    key := 20i32
    match collections.binary_search_by(&values, fn(v) { *v - key }) {
        some(index) => { assert_eq(index, 2usize) },
        none => { assert(false) },
    }
    people := [
        Person { id: 1, age: 36 },
        Person { id: 2, age: 21 },
        Person { id: 3, age: 45 },
    ]
    collections.stable_sort_by(&mut people, by_age)
    assert_eq(people[0].id, 2u32)
    limit := 40u32
    assert(collections.any(&people, fn(p) { p.age > limit }))
    assert(!collections.all(&people, fn(p) { p.age > limit }))
    assert_eq(collections.count(&people, fn(p) { p.age < limit }), 2usize)
    match collections.position(&people, fn(p) { p.age == 45 }) {
        some(index) => { assert_eq(index, 2usize) },
        none => { assert(false) },
    }
    ages := [0u32; 3]
    assert_eq(collections.map(&people, &mut ages, fn(p) { p.age }), 3usize)
    total := collections.fold(&ages, 0u32, fn(sum, age) { sum + *age })
    assert_eq(total, 102u32)
    seen := 0u32
    collections.for_each(&ages, fn(age) { seen += *age })
    assert_eq(seen, 102u32)
    collections.for_each_mut(&mut ages, fn(age) { *age += 1 })
    assert_eq(ages[0], 22u32)
    numbers := [1, 2, 3, 4, 5, 6]
    assert_eq(collections.partition_by(&mut numbers, fn(v) { *v % 2 == 0 }), 3usize)
}
"#);
}

#[test]
fn captured_bindings_keep_their_borrows() {
    rejects(
        "fn main() {\n    total: i32 = 0\n    add := fn(x: i32) { total += x }\n    total = 5\n    add(1)\n}",
        &["access conflicts with a live mutable borrow held by `add`"],
    );
    rejects(
        "fn main() {\n    total: i32 = 0\n    add := fn(x: i32) { total += x }\n    y := total\n    add(1)\n}",
        &["cannot read `total` while this borrow is live"],
    );
    rejects(
        "fn make() -> &dyn fn() -> i32 from(static) {\n    local: i32 = 3\n    return fn() { local }\n}\nfn main() {}",
        &["cannot return a borrow of a local value"],
    );
    rejects(
        "fn main() {\n    f: &dyn fn() -> i32 = fn() -> i32 { 0 }\n    {\n        inner: i32 = 5\n        f = fn() -> i32 { inner }\n    }\n    f()\n}",
        &["outlives its source"],
    );
    rejects(
        "fn apply(value: &mut i32, f: &mut dyn fn()) {}\nfn main() {\n    n: i32 = 0\n    apply(&mut n, fn() { n += 1 })\n}",
        &["overlapping borrows", "the closure mutably borrows `n`"],
    );
    rejects(
        "fn main() {\n    count: i32 = 0\n    f := fn() { count += 1 }\n    g := fn() { count += 2 }\n    f()\n    g()\n}",
        &["access conflicts with a live mutable borrow held by `f`"],
    );
    rejects(
        "fn main() {\n    count: i32 = 0\n    f := fn() { count += 1 }\n    g := f\n    f()\n}",
        &["has been moved"],
    );
}

#[test]
fn closure_types_follow_what_they_capture() {
    rejects(
        "fn run(f: &dyn fn()) { f() }\nfn main() {\n    count: i32 = 0\n    run(fn() { count += 1 })\n}",
        &["closure modifies captured `count`, so it is `&mut dyn fn()`"],
    );
    rejects(
        "fn run(f: fn()) { f() }\nfn main() {\n    count: i32 = 0\n    run(fn() { count += 1 })\n}",
        &["a function pointer `fn()` cannot capture local bindings"],
    );
    rejects(
        "fn run(f: &dyn fn()) { f() }\nfn main() {\n    n: i32 = 0\n    g := fn() { n += 1 }\n    run(g)\n}",
        &["expected `&dyn fn()`, found `&mut dyn fn()`"],
    );
    rejects(
        "fn main() {\n    let fixed: i32 = 1\n    f := fn() { fixed += 1 }\n}",
        &["closure modifies captured `fixed`, an immutable binding"],
    );
    rejects(
        "struct Token { id: i32 }\nfn consume(t: Token) {}\nfn main() {\n    token := Token { id: 1 }\n    f := fn() { consume(token) }\n}",
        &["a closure cannot move captured `token`"],
    );
    rejects(
        "fn main() {\n    zero: i32 = 0\n    saved: &i32 = &zero\n    keep := fn(r: &i32) { saved = r }\n}",
        &["a closure cannot store a borrow in captured `saved`"],
    );
    rejects(
        "fn main() {\n    n: i32 = 0\n    f := fn() -> &i32 { &n }\n}",
        &["returned borrow depends on `n`"],
    );
}

#[test]
fn function_values_are_checked_where_used() {
    rejects(
        "fn main() {\n    f := fn(x) { x }\n}",
        &["cannot infer the type of closure parameter `x`"],
    );
    rejects(
        "fn main() {\n    g := fn(a: i32) -> i32 { a }\n    g(1, 2)\n}",
        &["function value expects 1 arguments, found 2"],
    );
    rejects(
        "fn main() {\n    g: i32 = 3\n    g(1)\n}",
        &["`i32` is not a function value"],
    );
    rejects(
        "unsafe fn danger() {}\nfn main() {\n    f := danger\n}",
        &["unsafe function `danger` cannot be used as a function value"],
    );
    rejects(
        "fn id<T>(v: T) -> T { v }\nfn main() {\n    f := id\n}",
        &["generic function `id` needs a function type from its context"],
    );
    rejects(
        "fn main() {\n    n: i32 = 0\n    g := fn() -> i32 { n }\n    h := *g\n}",
        &["a closure reference cannot be dereferenced"],
    );
    rejects(
        "fn main() {\n    f: dyn fn() = fn() {}\n}",
        &["has no fixed size"],
    );
    rejects(
        "fn main() {\n    f := fn() {\n        r: i32!bool = ok(1)\n        v := r?\n    }\n}",
        &["`?` in a closure requires a declared Result return type"],
    );
    rejects(
        "fn main() {\n    f := fn() { 1 }\n    x: i32 = f()\n}",
        &["expected `i32`, found `isize`"],
    );
}

#[test]
fn closures_example_prints_its_summary() {
    let workspace = Workspace::new();
    let source = workspace.file("closures.dodo", include_str!("../examples/closures.dodo"));
    let executable = workspace
        .0
        .join(format!("example{}", std::env::consts::EXE_SUFFIX));
    let built = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(executable).output().unwrap();
    assert!(ran.status.success());
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "3 readings, warmest 35 C\n1 above 30 C\n95 308 \n"
    );
}
