//! Executable contracts for the resolved rules in language-spec-0.1.md.
//! Unsafe execution fixtures satisfy their allocation/aliasing preconditions;
//! accepting an unsafe program is not evidence that its pointers are valid.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-spec-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        path
    }
    fn compiler(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dodo"));
        command.current_dir(&self.0).args(args);
        command
    }
    fn run(&self, source: &Path, stdout: &[u8]) {
        for level in ["0", "3"] {
            let binary = self.0.join(format!("program-{level}"));
            success(
                self.compiler(&["build", "-O", level])
                    .arg(source)
                    .arg("-o")
                    .arg(&binary)
                    .output()
                    .unwrap(),
            );
            let output = Command::new(binary).output().unwrap();
            assert!(output.status.success(), "O{level}: {output:?}");
            assert_eq!(output.stdout, stdout, "O{level}");
        }
    }
    fn reject(&self, source: &str, diagnostic: &str) {
        let input = self.file("invalid.dodo", source);
        let result = self.compiler(&["check"]).arg(input).output().unwrap();
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(!result.status.success(), "accepted: {source}");
        assert!(
            stderr.contains(diagnostic),
            "expected {diagnostic:?}: {stderr}"
        );
        assert!(!stderr.contains("panicked at"), "{stderr}");
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    output
}
fn native(source: &str, stdout: &[u8]) {
    let workspace = Workspace::new();
    let input = workspace.file("main.dodo", source);
    workspace.run(&input, stdout);
}

#[test]
fn evaluation_order_and_precedence() {
    native(
        r#"package order
unsafe extern "C" fn putchar(ch: i32) -> i32
fn mark(ch: i32, value: i32) -> i32 { unsafe { putchar(ch) }; value }
fn take(a: i32, b: i32) -> i32 { a + b }
struct Pair { first: i32, second: i32
    fn call(&self, value: i32) -> i32 { self.first + value }
}
fn receiver(value: &Pair) -> &Pair { unsafe { putchar(71) }; value }
enum Both { Values(i32, i32) }
fn main() -> i32 {
    if mark(65, 1) + mark(66, 2) != 3 { return 1 }
    if take(mark(67, 1), mark(68, 2)) != 3 { return 2 }
    pair := Pair{second: mark(69, 2), first: mark(70, 1)}
    if receiver(&pair).call(mark(72, 2)) != 3 { return 3 }
    array := [mark(73, 1), mark(74, 2)]
    if array[0] + array[1] != 3 { return 4 }
    both := Both.Values(mark(75, 1), mark(76, 2))
    match both { Both.Values(a, b) => { if a + b != 3 { return 5 } } }
    if 20 - 6 - 3 != 11 { return 6 }
    if 1i32 << 2 + 1 != 8 { return 7 }
    if (1i32 | 2i32 ^ 3i32 & 1i32) != 3 { return 8 }
    if 2 + 3 * 4 != 14 { return 9 }
    if !true || true && false { return 10 }
    0
}
"#,
        b"ABCDEFGHIJKL",
    );
}

#[test]
fn assignment_captures_destination_and_old_value_before_rhs() {
    native(
        r#"package assignment
import "core/ptr"
unsafe extern "C" fn putchar(ch: i32) -> i32
fn index() -> usize { unsafe { putchar(65) }; 0 }
fn rhs() -> i32 { unsafe { putchar(66) }; 3 }
fn change(pointer: *mut i32) -> i32 { unsafe { ptr.write(pointer, 90) }; 2 }
struct Token { id: i32
    fn drop(&mut self) { unsafe { putchar(self.id) } }
}
fn replacement() -> Token { unsafe { putchar(67) }; Token{id: 69} }
fn main() -> i32 {
    values := [4i32]
    values[index()] += rhs()
    if values[0] != 7 { return 1 }
    values[index()] = rhs()
    if values[0] != 3 { return 2 }
    value := 5i32
    // The conversion's temporary borrow has ended; no checked loan is live.
    pointer := ptr.from_mut(&mut value)
    value += change(pointer)
    if value != 7 { return 3 }
    token := Token{id: 68}
    token = replacement()
    0
}
"#,
        b"ABABCDE",
    );
}

#[test]
fn scalar_representations_and_float_comparisons() {
    native(
        r#"package representation
import "core/mem"
import "core/ptr"
fn main() -> i32 {
    if mem.size_of::<bool>() != 1 || mem.align_of::<bool>() != 1 { return 1 }
    if mem.size_of::<i8>() != 1 || mem.size_of::<u16>() != 2 { return 2 }
    if mem.size_of::<i32>() != 4 || mem.size_of::<u64>() != 8 { return 3 }
    if mem.size_of::<f32>() != 4 || mem.size_of::<f64>() != 8 { return 4 }
    negative := -2i32
    // These raw reads access initialized scalar representations in live storage.
    unsafe {
        if ptr.read(ptr.from_ref(&negative) as *const u32) != 0xfffffffeu32 { return 5 }
        one := 1.0f32
        if ptr.read(ptr.from_ref(&one) as *const u32) != 0x3f800000u32 { return 6 }
        yes := true
        no := false
        if ptr.read(ptr.from_ref(&yes) as *const u8) != 1u8 { return 7 }
        if ptr.read(ptr.from_ref(&no) as *const u8) != 0u8 { return 8 }
    }
    zero := 0.0
    nan := zero / zero
    inf := 1.0 / zero
    if nan == nan || nan < zero || nan >= zero { return 9 }
    if !(nan != nan) || !(inf > 1.0) { return 10 }
    if -zero != zero || !(1.0 / -zero < zero) { return 11 }
    if (-7i32 / 3) != -2 || (-7i32 % 3) != -1 { return 12 }
    if (-8i32 >> 2) != -2 || (8u32 >> 2) != 2 { return 13 }
    if (255.75 as u8) != 255u8 || (-127.75 as i8) != -127i8 { return 14 }
    if (16777217u32 as f32) != 16777216f32 { return 15 }
    tiny := 1.17549435e-38f32 * 0.5f32
    unsafe { if ptr.read(ptr.from_ref(&tiny) as *const u32) != 0x00400000u32 { return 16 } }
    0
}
"#,
        b"",
    );
}

#[test]
fn constant_float_precision_is_documented_separately_from_runtime() {
    native(
        r#"package constant_float
// ID-FLOAT: the constant evaluator rounds via binary64. This integer lies
// just above a binary32 midpoint but rounds to that midpoint in binary64.
const ROUNDED:f32 = 9007199791611905u64 as f32
fn convert(value:u64)->f32 { value as f32 }
fn main()->i32 {
    if ROUNDED != 9007199254740992f32 { return 1 }
    if convert(9007199791611905u64) != 9007200328482816f32 { return 2 }
    0
}
"#,
        b"",
    );
    Workspace::new().reject(
        "package invalid\nconst X:f32 = (0f32 / 0f32) as f32\n",
        "constant floating conversion",
    );
}

#[test]
fn checked_float_cast_boundaries_trap() {
    let workspace = Workspace::new();
    for expression in [
        "-0.5 as u8",
        "-128.5 as i8",
        "256.0 as u8",
        "(0.0 / 0.0) as i32",
        "(1.0 / 0.0) as i32",
        "(0.0 / 0.0) as f32",
        "(1.0 / 0.0) as f32",
        "3.5e38 as f32",
    ] {
        let source = workspace.file(
            "trap.dodo",
            &format!("package trap\nfn main() {{ value := {expression}\ncore.drop(value) }}\n"),
        );
        for level in ["0", "3"] {
            let binary = workspace.0.join("trap-program");
            success(
                workspace
                    .compiler(&["build", "-O", level])
                    .arg(&source)
                    .arg("-o")
                    .arg(&binary)
                    .output()
                    .unwrap(),
            );
            let result = Command::new(&binary).output().unwrap();
            assert!(
                !result.status.success(),
                "{expression} did not trap at O{level}"
            );
        }
    }
}

#[test]
fn aggregate_layout_tags_and_string_byte_lengths() {
    native(
        r#"package layout
import "core/mem"
import "core/ptr"
struct Plain { a: u8, b: u32, c: u16 }
@repr(C)
struct CLayout { a: u8, b: u32, c: u16 }
struct Empty {}
enum State { Empty, Byte(u8), Word(u32) }
fn main() -> i32 {
    if mem.size_of::<Plain>() != 12 || mem.align_of::<Plain>() != 4 { return 1 }
    if mem.size_of::<CLayout>() != 12 || mem.offset_of::<CLayout>("b") != 4 { return 2 }
    if mem.offset_of::<Plain>("a") != 0 || mem.offset_of::<Plain>("c") != 8 { return 3 }
    if mem.size_of::<[3]Plain>() != 36 || mem.size_of::<[0]Plain>() != 0 { return 4 }
    if mem.align_of::<[0]Plain>() != 4 || mem.size_of::<Empty>() != 0 { return 5 }
    if mem.size_of::<MaybeUninit<Plain>>() != 12 || mem.align_of::<MaybeUninit<Plain>>() != 4 { return 6 }
    if mem.size_of::<void>() != 0 || mem.align_of::<void>() != 1 { return 7 }
    if mem.size_of::<&str>() != 2 * mem.size_of::<usize>() { return 8 }
    if mem.size_of::<&[u8]>() != mem.size_of::<&str>() { return 9 }
    if mem.size_of::<&i32>() != mem.size_of::<*mut i32>() { return 10 }
    if mem.size_of::<Option<u32>>() != 8 || mem.size_of::<u16!u32>() != 8 { return 11 }
    if mem.size_of::<void!u8>() != 3 || mem.size_of::<State>() != 12 { return 12 }
    state := State.Word(42u32)
    present := some(9u32)
    absent: Option<u32> = none
    // Read only active tags, never padding or inactive payload slots.
    unsafe {
        if ptr.read(ptr.from_ref(&state) as *const u32) != 2u32 { return 13 }
        if ptr.read(ptr.from_ref(&present) as *const u8) != 1u8 { return 14 }
        if ptr.read(ptr.from_ref(&absent) as *const u8) != 0u8 { return 15 }
    }
    result: u16!u32 = err(7u32)
    tag := unsafe { ptr.read(ptr.from_ref(&result) as *const u8) }
    match result { ok(_) => { return 17 }, err(value) => { if value != 7u32 { return 18 } } }
    if tag != 1u8 { return 16 }
    text := "é\0"
    if text.len != 3 || b"a\0".len != 2 { return 19 }
    bytes := mem.str_bytes(text)
    if bytes[0] != 0xc3u8 || bytes[1] != 0xa9u8 || bytes[2] != 0u8 { return 20 }
    0
}
"#,
        b"",
    );
}

#[test]
fn pointer_integer_round_trip_offsets_and_byte_access() {
    native(
        r#"package pointers
import "core/ptr"
fn main() -> i32 {
    values := [11u32, 22u32, 33u32]
    pointer := ptr.as_mut_ptr(&mut values)
    unsafe {
        address := pointer as usize
        restored := address as *mut u32
        if (restored as usize) != address { return 1 }
        if restored != pointer { return 6 }
        // Raw address casts preserve bits; unlike numeric casts they do not trap.
        if ((-1i8 as *const u8) as usize) != 255usize { return 8 }
        if ((0x1234usize as *const u8) as u8) != 0x34u8 { return 9 }
        byte_address := (address + 4) as *const u32
        if ptr.read(byte_address) != 22u32 { return 7 }
        // The integer preserves the address of this still-live allocation.
        last := ptr.offset(restored, 2)
        end := ptr.offset(restored, 3)
        if ptr.read(ptr.offset(end, -1)) != 33u32 { return 2 }
        ptr.write(last, 44u32)
        view := ptr.borrow_slice(pointer, 3, &values)
        if view[2] != 44u32 { return 3 }
        null := 0usize as *mut u32
        if !ptr.is_null(null) { return 4 }
        ptr.copy(null, null, 0)
        ptr.copy_nonoverlapping(null, null, 0)
        ptr.write_bytes(null, 0u8, 0)
        // The unaligned byte range stays within one initialized array.
        bytes := [0u8; 6]
        unaligned := ptr.offset(ptr.as_mut_ptr(&mut bytes), 1) as *mut u32
        ptr.write_unaligned(unaligned, 0x12345678u32)
        if ptr.read_unaligned(unaligned) != 0x12345678u32 { return 5 }
    }
    0
}
"#,
        b"",
    );
}

#[test]
fn owner_view_arguments_keep_their_effects_and_order() {
    native(
        r#"package view_order
import "core/ptr"
unsafe extern "C" fn putchar(ch:i32)->i32
fn pointer(values:&[u32])->*const u32 { unsafe { putchar(65) }; ptr.as_ptr(values) }
fn count()->usize { unsafe { putchar(66) }; 2 }
fn owner(values:&[u32])->&[u32] { unsafe { putchar(67) }; values }
fn main()->i32 {
    values := [11u32, 22u32]
    view := unsafe { ptr.borrow_slice(pointer(&values), count(), owner(&values)) }
    if view[0] != 11u32 || view[1] != 22u32 { return 1 }
    0
}
"#,
        b"ABC",
    );
}

#[test]
fn unsafe_pointer_conversions_and_unsupported_abi_are_rejected() {
    let workspace = Workspace::new();
    for (body, message) in [
        ("fn main(){ p := 0usize as *const u8 }", "unsafe"),
        (
            "fn main(){ x:=1i32\np:= &x as *mut i32 }",
            "shared reference",
        ),
        (
            "fn main(){ unsafe { p:=0usize as *const i32\nr:=p as &i32 } }",
            "explicit lifetime primitive",
        ),
        ("struct S { x:i32 }\nextern \"C\" fn take(x:S)", "C ABI"),
        ("extern \"C\" fn take(x:&i32)", "C ABI"),
        ("extern \"C\" fn take(x:&[u8])", "C ABI"),
        ("extern \"C\" fn take(x:Option<i32>)", "C ABI"),
        ("extern \"Rust\" fn take()", "only the"),
        ("extern \"C\" fn take(x:i32, ...)", "expected"),
        ("@packed struct S { x:i32 }", "unknown attribute"),
        ("enum E { A = 3 }", "expected"),
    ] {
        workspace.reject(&format!("package invalid\n{body}\n"), message);
    }
}

#[test]
fn c_scalar_results_and_struct_pointer_layout_interoperate() {
    let workspace = Workspace::new();
    // An independent compiler supplies the target C ABI and struct layout.
    let foreign = workspace.file("foreign.rs", r#"
#[repr(C)] pub struct Layout { a:u8, b:u32, c:u16 }
#[no_mangle] pub unsafe extern "C" fn update(p:*mut Layout, b:bool, s:i8, u:u16, x:f32, y:f64)->i8 {
    if std::mem::size_of::<Layout>() != 12 || !b || s != -128 || u != 65535 || x != 1.5 || y != 2.25 { return 1; }
    if (*p).a != 7 || (*p).b != 42 || (*p).c != 9 { return 2; }
    (*p).b = 99;
    -128
}
#[no_mangle] pub extern "C" fn truth()->bool { true }
#[no_mangle] pub extern "C" fn high()->u16 { 65535 }
#[no_mangle] pub extern "C" fn fraction()->f64 { 2.25 }
"#);
    let object = workspace.0.join("foreign.o");
    success(
        Command::new("rustc")
            .args([
                "--edition=2021",
                "--crate-type=lib",
                "--emit=obj",
                "-Copt-level=3",
            ])
            .arg(foreign)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    let source = workspace.file("ffi.dodo", r#"package ffi
import "core/ptr"
@repr(C)
struct Layout { a:u8, b:u32, c:u16 }
unsafe extern "C" fn update(p:*mut Layout,b:bool,s:i8,u:u16,x:f32,y:f64)->i8
unsafe extern "C" fn truth()->bool
unsafe extern "C" fn high()->u16
unsafe extern "C" fn fraction()->f64
fn main()->i32 {
    value := Layout{a:7u8,b:42u32,c:9u16}
    unsafe {
        if update(ptr.from_mut(&mut value),true,-128i8,65535u16,1.5f32,2.25f64) != -128i8 { return 1 }
        if !truth() || high() != 65535u16 || fraction() != 2.25f64 { return 2 }
    }
    if value.b != 99u32 { return 3 }
    0
}
"#);
    for level in ["0", "3"] {
        let binary = workspace.0.join("ffi-program");
        success(
            workspace
                .compiler(&["build", "-O", level])
                .arg(&source)
                .arg("--link-arg")
                .arg(&object)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap(),
        );
        success(Command::new(binary).output().unwrap());
    }
}

#[test]
fn incompatible_lowered_foreign_declarations_fail_before_linking() {
    let workspace = Workspace::new();
    workspace.file("a.dodo", "package a\nextern \"C\" fn same(x:i32)\n");
    workspace.file("b.dodo", "package b\nextern \"C\" fn same(x:f64)\n");
    let source = workspace.file(
        "main.dodo",
        "package main\nimport \"a\"\nimport \"b\"\nfn main() {}\n",
    );
    let output = workspace
        .compiler(&["build", "--emit", "llvm-ir"])
        .arg(source)
        .arg("-o")
        .arg(workspace.0.join("invalid.ll"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("incompatible declarations of external symbol")
    );
}

#[test]
fn directory_package_scope_aliases_and_constant_initialization() {
    let workspace = Workspace::new();
    workspace.file("app/a.dodo", "package app\nimport \"dep\" as helper\nconst ANSWER:i32 = LATER + helper.VALUE\nfn main()->i32 { answer() - 42 }\n");
    workspace.file(
        "app/z.dodo",
        "package app\nconst LATER:i32 = 2\nfn answer()->i32 { ANSWER }\n",
    );
    workspace.file(
        "app/dep/main.dodo",
        "package dep\npub const VALUE:i32 = 40\n",
    );
    workspace.file("app/ignored.txt", "invalid");
    workspace.run(&workspace.0.join("app"), b"");
    workspace.file("app/z.dodo", "package app\nimport \"dep\" as different\n");
    let output = workspace.compiler(&["check", "app"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicting aliases"));
    workspace.reject(
        "package invalid\nfn compute()->i32 { 1 }\nconst X:i32 = compute()\n",
        "constant",
    );
}

#[test]
fn static_storage_has_no_implicit_initialization_or_destruction_hooks() {
    native(
        r#"package static_storage
unsafe extern "C" fn putchar(ch:i32)->i32
struct Token { id:i32
    fn drop(&mut self) { unsafe { putchar(self.id) } }
}
static mut VALUE:Token = Token{id:65}
fn init() { unsafe { putchar(73) } }
fn main()->i32 { unsafe { VALUE.id - 65 } }
"#,
        b"",
    );
}

#[test]
fn package_paths_cycles_ambiguity_and_unit_mismatch_are_rejected() {
    let workspace = Workspace::new();
    workspace.reject("package invalid\nimport \"\"\n", "cannot be empty");
    for path in ["/absolute", "./dep", "../dep", "a//b", "a/../b", "a/"] {
        workspace.reject(
            &format!("package invalid\nimport {path:?}\n"),
            "invalid import",
        );
    }
    workspace.file("a.dodo", "package a\nimport \"b\"\n");
    workspace.file("b.dodo", "package b\nimport \"a\"\n");
    for (expected, replacement) in [
        ("cyclic", None),
        ("ambiguous", Some(("b/main.dodo", "package b\n"))),
    ] {
        if let Some((path, text)) = replacement {
            workspace.file(path, text);
        }
        let output = workspace.compiler(&["check", "a.dodo"]).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
    fs::remove_file(workspace.0.join("b.dodo")).unwrap();
    workspace.file("b/main.dodo", "package wrong\n");
    let output = workspace.compiler(&["check", "a.dodo"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected `b`"));
    workspace.file("unit/a.dodo", "package first\n");
    workspace.file("unit/b.dodo", "package second\n");
    let output = workspace.compiler(&["check", "unit"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("package"));
}

#[test]
fn target_profiles_publish_width_endianness_and_native_symbols() {
    let workspace = Workspace::new();
    let source = workspace.file(
        "library.dodo",
        "package profile\nimport \"core/mem\"\npub fn width()->usize { mem.size_of::<usize>() }\n",
    );
    for (target, bits, bytes, endian) in [
        ("x86_64-unknown-linux-gnu", 64, 8, "e"),
        ("i686-unknown-linux-gnu", 32, 4, "e"),
        ("powerpc64-unknown-linux-gnu", 64, 8, "E"),
    ] {
        let path = workspace.0.join("profile.ll");
        success(
            workspace
                .compiler(&["build", "--emit", "llvm-ir", "--target", target])
                .arg(&source)
                .arg("-o")
                .arg(&path)
                .output()
                .unwrap(),
        );
        let ir = fs::read_to_string(path).unwrap();
        assert!(
            ir.contains(&format!("target datalayout = \"{endian}")),
            "{ir}"
        );
        assert!(ir.contains(&format!("ret i{bits} {bytes}")), "{ir}");
        assert!(
            ir.contains(&format!("define i{bits} @dodo.profile.width()")),
            "{ir}"
        );
        assert!(
            !ir.contains("@main("),
            "library emission added hosted startup"
        );
    }
}

#[test]
fn hosted_entry_signatures_and_c_definitions() {
    native("package entry\nfn main() {}\n", b"");
    native(
        "package entry\nextern \"C\" fn answer()->i32 { 42 }\nfn main()->i32 { unsafe { answer() - 42 } }\n",
        b"",
    );
    let workspace = Workspace::new();
    for source in [
        "package entry\nfn helper() {}\n",
        "package entry\nfn main(x:i32) {}\n",
        "package entry\nfn main()->u32 { 0u32 }\n",
    ] {
        let input = workspace.file("entry.dodo", source);
        // These are valid libraries, but cannot supply a hosted entry point.
        success(workspace.compiler(&["check"]).arg(&input).output().unwrap());
        let output = workspace
            .compiler(&["build"])
            .arg(&input)
            .arg("-o")
            .arg(workspace.0.join("entry"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("main"));
    }
}

#[test]
fn lexical_boundaries_and_literal_encodings() {
    native(
        "package lexical\r\n// CRLF keeps statement boundaries.\r\nfn main()->i32 {\r\n\
         lower_case := 0x_f_f_u16\r\nUPPER_CASE := 0b10__01u16\r\n\
         if lower_case + UPPER_CASE != 264u16 { return 1 }\r\n\
         if b'\\xFF' != 255u8 || b\"\\0\\n\\r\\t\\\\\\\"\\'\".len != 7 { return 2 }\r\n\
         if \"\\xC3\\xA9\\u{1f600}\".len != 6 { return 3 }\r\n\
         if 1_2.5_0e-1f64 != 1.25 { return 4 }\r\n0\r\n}\r\n",
        b"",
    );
    let workspace = Workspace::new();
    for (body, message) in [
        ("fn main(){ café:=1 }", "unexpected character"),
        ("fn main(){ x:=b\"é\" }", "ASCII"),
        ("fn main(){ x:=\"\\xFF\" }", "UTF-8"),
        ("fn main(){ x:=\"\\u{d800}\" }", "Unicode scalar"),
        ("fn main(){ x:=\"\\q\" }", "unknown escape"),
        ("fn main(){ x:=b'ab' }", "exactly one byte"),
        ("fn main(){ x:=0b2 }", "expected digits"),
        ("fn main(){ x:=1e+ }", "exponent"),
        ("fn main(){ x:=1.0u32 }", "integer suffix"),
        ("fn main(){ x:=3.5e38f32 }", "out of range"),
        ("fn main(){ x:=18446744073709551616 }", "u64 range"),
        ("fn main(){ if 1 {} }", "bool"),
        ("fn main(){ let for = 1 }", "expected"),
        ("/* block */", "expected"),
    ] {
        workspace.reject(&format!("package lexical\n{body}\n"), message);
    }
    workspace.reject("\u{feff}package lexical\n", "unexpected character");
    let path = workspace.0.join("non_utf8.dodo");
    fs::write(&path, b"package lexical\n// \xff\n").unwrap();
    let output = workspace.compiler(&["check"]).arg(path).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("UTF-8"));
}

#[cfg(unix)]
#[test]
fn canonical_package_identity_and_symlink_enumeration() {
    use std::os::unix::fs::symlink;
    let workspace = Workspace::new();
    workspace.file("dep.dodo", "package dep\nstatic mut COUNT:i32 = 0\npub fn next()->i32 { unsafe { COUNT += 1\nCOUNT } }\n");
    fs::create_dir(workspace.0.join("other")).unwrap();
    symlink(
        workspace.0.join("dep.dodo"),
        workspace.0.join("other/dep.dodo"),
    )
    .unwrap();
    let input = workspace.file("main.dodo", "package app\nimport \"dep\" as first\nimport \"other/dep\" as second\nfn main()->i32 { first.next() + second.next() - 3 }\n");
    workspace.run(&input, b"");
    let wrong = workspace.file("wrong.dodo", "package wrong\n");
    workspace.file("unit/a.dodo", "package unit\nfn main() {}\n");
    symlink(&wrong, workspace.0.join("unit/z.dodo")).unwrap();
    workspace.run(&workspace.0.join("unit"), b"");
    // Explicit file input follows the link, although enumeration excluded it.
    success(
        workspace
            .compiler(&["check", "unit/z.dodo"])
            .output()
            .unwrap(),
    );
}
