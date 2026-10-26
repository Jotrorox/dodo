//! Interfaces, bounds, and `dyn` references: native execution at both
//! optimization levels, and the diagnostics for types that do not fit.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-interfaces-{}-{}",
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

const SHAPES: &str = r#"
interface Shape {
    fn area(&self) -> u32
    fn scale(&mut self, factor: u32)
}

struct Square {
    side: u32

    pub fn area(&self) -> u32 {
        self.side * self.side
    }
    pub fn scale(&mut self, factor: u32) {
        self.side *= factor
    }
}

struct Rect: Shape {
    w: u32
    h: u32

    pub fn area(&self) -> u32 {
        self.w * self.h
    }
    pub fn scale(&mut self, factor: u32) {
        self.w *= factor
        self.h *= factor
    }
}
"#;

#[test]
fn bounded_generics_and_dyn_references_run() {
    run(&format!(
        r#"{SHAPES}
fn doubled<S: Shape>(shape: &mut S) -> u32 {{
    shape.scale(2)
    shape.area()
}}

fn total(shapes: &[&dyn Shape]) -> u32 {{
    sum := 0u32
    for shape in shapes {{
        sum += shape.area()
    }}
    sum
}}

struct Canvas {{
    focus: &mut dyn Shape
    hint: Option<&dyn Shape>

    fn grow(&mut self) -> u32 {{
        self.focus.scale(3)
        self.focus.area()
    }}
}}

fn pick(first: bool, a: &dyn Shape, b: &dyn Shape) -> &dyn Shape from(a, b) {{
    if first {{
        return a
    }}
    b
}}

struct Holder<S: Shape> {{
    shape: S

    fn area(&self) -> u32 {{
        self.shape.area()
    }}
}}

fn main() {{
    s := Square {{ side: 3 }}
    r := Rect {{ w: 2, h: 5 }}
    core.assert_eq(doubled(&mut s), 36u32)
    {{
        shapes: [2]&dyn Shape = [&s, &r]
        core.assert_eq(total(&shapes), 46u32)
        // A dyn type implements its own interface, so it satisfies bounds.
        core.assert_eq(doubled_view(shapes[1]), 10u32)
        core.assert_eq(pick(false, shapes[0], shapes[1]).area(), 10u32)
    }}
    {{
        canvas := Canvas {{ focus: &mut r, hint: some(&s) }}
        core.assert_eq(canvas.grow(), 90u32)
        if let some(hint) = &canvas.hint {{
            core.assert_eq(hint.area(), 36u32)
        }}
    }}
    mutable: &mut dyn Shape = &mut s
    doubled(mutable)
    shared: &dyn Shape = mutable
    core.assert_eq(shared.area(), 144u32)
    explicit := &r as &dyn Shape
    core.assert_eq(explicit.area(), 90u32)
    h := Holder {{ shape: Square {{ side: 4 }} }}
    core.assert_eq(h.area(), 16u32)
}}

fn doubled_view<S: Shape>(shape: &S) -> u32 {{
    shape.area()
}}
"#
    ));
}

#[test]
fn inherited_methods_large_values_and_borrowed_results_dispatch() {
    run(r#"
interface Named {
    fn name(&self) -> &str
}

interface Sensor: Named {
    fn read(&mut self) -> i32
    fn label(&self) -> &[u8]
}

interface Bulk {
    fn fill(&self, value: u8) -> [2048]u8
    fn sum(&self, data: [2048]u8) -> u32
}

// Public and in the root package: its large values pass differently from an
// internal function, so the method table holds adapters.
pub struct Filler {
    base: u8

    pub fn fill(&self, value: u8) -> [2048]u8 {
        [value + self.base; 2048]
    }
    pub fn sum(&self, data: [2048]u8) -> u32 {
        total := 0u32
        for x in data {
            total += *x as u32
        }
        total
    }
}

struct Thermometer: Sensor {
    celsius: i32
    tag: [3]u8

    pub fn name(&self) -> &str {
        "thermometer"
    }
    pub fn read(&mut self) -> i32 {
        self.celsius += 1
        self.celsius
    }
    pub fn label(&self) -> &[u8] {
        &self.tag
    }
}

struct Counter<T> {
    step: T
    count: i32

    pub fn name(&self) -> &str {
        "counter"
    }
    pub fn read(&mut self) -> i32 {
        self.count += 10
        self.count
    }
    pub fn label(&self) -> &[u8] {
        b"ctr"
    }
}

fn poll(sensors: &mut [&mut dyn Sensor]) -> i32 {
    total := 0i32
    for sensor in sensors {
        total += sensor.read()
    }
    total
}

fn main() {
    t := Thermometer { celsius: 20, tag: [1, 2, 3] }
    c := Counter<u8> { step: 1, count: 0 }
    {
        sensors: [2]&mut dyn Sensor = [&mut t, &mut c]
        core.assert_eq(poll(&mut sensors), 31i32)
        // The first parent's methods begin the table: an upcast reuses it.
        named: &dyn Named = &*sensors[1]
        core.assert_eq(named.name(), "counter")
    }
    view: &dyn Sensor = &t
    label := view.label()
    core.assert_eq(label[2], 3u8)
    core.assert_eq(view.name(), "thermometer")
    f := Filler { base: 1 }
    bulk: &dyn Bulk = &f
    data := bulk.fill(2)
    core.assert_eq(bulk.sum(data), 6144u32)
}
"#);
}

#[test]
fn standard_library_interfaces_accept_dyn_streams_and_devices() {
    run(r#"
import "std/io"
import "std/embedded/hal"
import "std/embedded/hal/fake"

fn copy_all(input: &mut dyn io.Reader, output: &mut dyn io.Writer) -> usize!io.Error {
    buffer: [4]u8 = [0; 4]
    total := 0usize
    for {
        count := io.read_once(input, &mut buffer)?
        if count == 0 {
            return ok(total)
        }
        total += io.write_all(output, &buffer[..count])?
    }
}

fn toggle_all(pins: &mut [&mut dyn hal.OutputPin]) {
    for pin in pins {
        pin.set_high()
        pin.set_low()
        pin.set_high()
    }
}

fn main() {
    input := io.MemoryReader.new(b"interfaces!")
    storage: [16]u8 = [0; 16]
    output := io.MemoryWriter.new(&mut storage)
    core.assert_eq(copy_all(&mut input, &mut output)!, 11usize)
    first := fake.Pin.new()
    second := fake.Pin.new()
    {
        pins: [2]&mut dyn hal.OutputPin = [&mut first, &mut second]
        toggle_all(&mut pins)
    }
    core.assert_eq(first.changes(), 3u32)
    core.assert(second.is_high())
    bus := fake.I2c.new(0x40)
    {
        device: &mut dyn hal.I2c = &mut bus
        hal.write_register(device, 0x40, 1, 2)!
        core.assert_eq(hal.read_register(device, 0x40, 1)!, 2u8)
    }
    core.assert_eq(bus.register(1), 2u8)
}
"#);
}

#[test]
fn interfaces_cross_packages_with_their_visibility() {
    let workspace = Workspace::new();
    workspace.file(
        "sensors/sensors.dodo",
        "package sensors\n\
         pub interface Sensor {\n    fn read(&mut self) -> i32\n}\n\
         pub fn twice<S: Sensor>(sensor: &mut S) -> i32 {\n    sensor.read() + sensor.read()\n}\n\
         pub fn once(sensor: &mut dyn Sensor) -> i32 {\n    sensor.read()\n}\n",
    );
    let source = workspace.file(
        "main.dodo",
        "package main\nimport \"sensors\"\n\
         struct Fixed {\n    value: i32\n    pub fn read(&mut self) -> i32 { self.value }\n}\n\
         struct Local: sensors.Sensor {\n    value: i32\n    fn read(&mut self) -> i32 { self.value }\n}\n\
         fn main() {\n    f := Fixed { value: 4 }\n    core.assert_eq(sensors.twice(&mut f), 8i32)\n\
         \x20   core.assert_eq(sensors.once(&mut f), 4i32)\n\
         \x20   // A private method may implement an interface where it is visible.\n\
         \x20   l := Local { value: 2 }\n    s: &mut dyn sensors.Sensor = &mut l\n\
         \x20   core.assert_eq(sensors.once(s), 2i32)\n}\n",
    );
    run_in(&workspace, &source);
    let private = workspace.file(
        "main.dodo",
        "package main\nimport \"sensors\"\n\
         struct Hidden {\n    value: i32\n    fn read(&mut self) -> i32 { self.value }\n}\n\
         fn main() {\n    h := Hidden { value: 1 }\n    sensors.twice(&mut h)\n}\n",
    );
    let (success, output) = check(&workspace, &private);
    assert!(!success);
    for message in [
        "`Hidden` does not implement interface `sensors.Sensor`",
        "method `read` is private to its package",
        "`sensors.twice` requires `S: sensors.Sensor`",
    ] {
        assert!(
            output.contains(message),
            "missing `{message}` in:\n{output}"
        );
    }
}

#[test]
fn types_that_do_not_fit_are_reported_where_they_are_used() {
    rejects(
        &format!(
            r#"{SHAPES}
struct Circle {{
    radius: u32
    pub fn area(&self) -> u64 {{
        3 * (self.radius as u64)
    }}
}}
fn doubled<S: Shape>(shape: &mut S) -> u32 {{
    shape.scale(2)
    shape.area()
}}
fn main() {{
    c := Circle {{ radius: 1 }}
    doubled(&mut c)
}}"#
        ),
        &[
            "`Circle` does not implement interface `Shape`",
            "`Circle` is used as `S` here",
            "`doubled` requires `S: Shape`",
            "method `area` has signature `fn area(&self) -> u64`, but `Shape` requires `fn area(&self) -> u32`",
            "missing method `fn scale(&mut self, factor: u32)`",
        ],
    );
    rejects(
        &format!(
            "{SHAPES}\nstruct Dot {{ x: u32 }}\nfn main() {{\n d := Dot {{ x: 1 }}\n s: &dyn Shape = &d\n}}"
        ),
        &[
            "`Dot` does not implement interface `Shape`",
            "`&Dot` is converted to `&dyn Shape` here",
        ],
    );
    rejects(
        &format!(
            "{SHAPES}\nstruct Wrong: Shape {{\n x: u32\n pub fn area(&mut self) -> u32 {{ self.x }}\n pub fn scale(&mut self, factor: u32) {{}}\n}}\nfn main() {{}}"
        ),
        &[
            "`Wrong` declares that it implements `Shape`",
            "method `area` has signature `fn area(&mut self) -> u32`",
        ],
    );
    rejects(
        &format!(
            "{SHAPES}\nstruct Bad<T>: Shape {{\n value: T\n pub fn area(&self) -> u32 {{ 1 }}\n}}\nfn main() {{\n b := Bad<u8> {{ value: 1 }}\n}}"
        ),
        &["missing method `fn scale(&mut self, factor: u32)`"],
    );
    rejects(
        &format!(
            "{SHAPES}\nstruct Holder<S: Shape> {{ shape: S }}\nfn main() {{\n h := Holder {{ shape: 3u32 }}\n}}"
        ),
        &[
            "`u32` does not implement interface `Shape`",
            "`u32` has no methods",
        ],
    );
}

#[test]
fn dyn_references_keep_their_restrictions() {
    let cases: &[(&str, &[&str])] = &[
        (
            "fn main() {\n s := Square { side: 1 }\n v: dyn Shape = s\n}",
            &["`dyn Shape` has no fixed size"],
        ),
        (
            "fn f(shapes: &[dyn Shape]) {}\nfn main() {}",
            &["`dyn Shape` has no fixed size"],
        ),
        (
            "fn main() {\n s := Square { side: 1 }\n v: &dyn Shape = &s\n w: &mut dyn Shape = v\n}",
            &["expected `&mut dyn Shape`, found `&dyn Shape`"],
        ),
        (
            "fn main() {\n s := Square { side: 1 }\n v: &dyn Shape = &s\n v.scale(2)\n}",
            &["cannot mutably borrow"],
        ),
        (
            "fn make() -> &dyn Shape from(static) {\n s := Square { side: 1 }\n return &s\n}\nfn main() {}",
            &["local"],
        ),
        (
            "struct Keep { shape: &dyn Shape }\nfn main() {\n k := Keep { shape: &Square { side: 1 } }\n}",
            &["cannot borrow a temporary"],
        ),
        (
            "fn main() {\n s := Square { side: 1 }\n v: &dyn Shape = &s\n s.side = 2\n core.assert_eq(v.area(), 4u32)\n}",
            &["borrow"],
        ),
        (
            "fn main() {\n s := Square { side: 1 }\n v: &dyn Shape = &s\n u := *v\n}",
            &["move"],
        ),
    ];
    for (body, messages) in cases {
        rejects(&format!("{SHAPES}\n{body}"), messages);
    }
}

#[test]
fn interface_declarations_are_validated() {
    let cases: &[(&str, &[&str])] = &[
        (
            "interface Make {\n fn new() -> Self\n}\nfn f(m: &dyn Make) {}\nfn main() {}",
            &[
                "interface `Make` cannot be used as `dyn Make`",
                "method `new` has no `self` receiver",
            ],
        ),
        (
            "interface Same {\n fn same(&self, other: &Self) -> bool\n}\nfn f(m: &dyn Same) {}\nfn main() {}",
            &["uses `Self` outside its receiver"],
        ),
        (
            "interface A {\n fn f(&self) -> u8\n}\ninterface B {\n fn f(&self) -> u16\n}\ninterface C: A + B {}\nfn main() {}",
            &["interface `C` inherits two different `f` methods"],
        ),
        (
            "interface A: B {}\ninterface B: A {}\nfn main() {}",
            &["includes itself through its parents"],
        ),
        (
            "interface A {\n fn f(&self)\n}\nstruct A {}\nfn main() {}",
            &["duplicate declaration `A`"],
        ),
        (
            "struct S {}\nfn f<T: S>(x: &T) {}\nfn main() {}",
            &["unknown interface `S`", "`S` is a concrete type"],
        ),
        (
            "interface A {\n fn f(&self)\n}\nfn f<T>(x: &T) {}\nfn g<U: A>(x: &U) {}\nstruct W<T: A> { v: T }\nfn h(x: &dyn Missing) {}\nfn main() {}",
            &["unknown interface `Missing`"],
        ),
        (
            "interface A {\n fn f(&self) { }\n}\nfn main() {}",
            &["interface methods are signatures without a body"],
        ),
        (
            "interface A {\n fn drop(&mut self)\n}\nfn main() {}",
            &["interfaces cannot declare `drop`"],
        ),
        (
            "interface A {\n fn f<T>(&self, x: T)\n}\nfn main() {}",
            &["interface methods cannot have type parameters"],
        ),
        (
            "interface A {\n fn view(&self, other: &[u8]) -> &[u8] from(self)\n}\nstruct V {\n data: [2]u8\n pub fn view(&self, other: &[u8]) -> &[u8] from(self, other) { other }\n}\nfn main() {\n v := V { data: [1, 2] }\n a: &dyn A = &v\n}",
            &["returns a borrow `from(self, other)`, but `A` only allows `from(self)`"],
        ),
        (
            "interface A {\n fn f(&self)\n}\nstruct U {\n pub unsafe fn f(&self) {}\n}\nfn main() {\n u := U {}\n a: &dyn A = &u\n}",
            &["method `f` is unsafe, but `A` declares it safe"],
        ),
        (
            "interface A {\n fn f(&self) -> u8\n}\ninterface B: A {\n fn g(&self) -> u8\n}\ninterface C {\n fn h(&self) -> u8\n}\ninterface D: C + B {}\nstruct S {\n pub fn f(&self) -> u8 { 1 }\n pub fn g(&self) -> u8 { 2 }\n pub fn h(&self) -> u8 { 3 }\n}\nfn main() {\n s := S {}\n d: &dyn D = &s\n b: &dyn B = d\n}",
            &["cannot convert `&dyn D` to `&dyn B`"],
        ),
        (
            "pub interface Open: Hidden {}\ninterface Hidden {\n fn f(&self)\n}\nfn main() {}",
            &["public interface extends private interface `Hidden`"],
        ),
        (
            "interface Hidden {\n fn f(&self)\n}\npub fn f(x: &dyn Hidden) {}\nfn main() {}",
            &["public API exposes private interface `Hidden`"],
        ),
    ];
    for (body, messages) in cases {
        rejects(body, messages);
    }
}

#[test]
fn unsafe_interface_methods_need_unsafe_calls() {
    rejects(
        "interface Poke {\n unsafe fn poke(&mut self)\n}\nstruct P {\n pub unsafe fn poke(&mut self) {}\n}\nfn main() {\n p := P {}\n d: &mut dyn Poke = &mut p\n d.poke()\n}",
        &["requires an explicit unsafe block"],
    );
    run(
        "interface Poke {\n unsafe fn poke(&mut self) -> u8\n}\nstruct P {\n pub fn poke(&mut self) -> u8 { 7 }\n}\nfn main() {\n p := P {}\n d: &mut dyn Poke = &mut p\n unsafe {\n core.assert_eq(d.poke(), 7u8)\n }\n}",
    );
}
