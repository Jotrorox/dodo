//! Checked owner views and shareable allocator capabilities. These regressions
//! distinguish access to an owner's storage from the sources it keeps alive.
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-owner-{}-{}",
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
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn rejects(source: &str, expected: &str) {
    let workspace = Workspace::new();
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("check")
        .arg(workspace.source(source))
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "accepted invalid source:\n{source}"
    );
    assert!(error.contains(expected), "expected {expected:?}:\n{error}");
    assert!(!error.contains("panicked"), "{error}");
}
fn native(source: &str) {
    let workspace = Workspace::new();
    let input = workspace.source(source);
    for level in ["0", "3"] {
        let exe = workspace
            .0
            .join(format!("run-O{level}{}", std::env::consts::EXE_SUFFIX));
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&input)
            .args(["-O", level, "-o"])
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "O{level}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(exe).output().unwrap();
        assert_eq!(output.status.code(), Some(0), "O{level}: {output:?}");
    }
}

#[test]
fn simultaneous_boxes_destruction_failure_alignment_and_zst() {
    native(include_str!("stdlib/shared_storage.dodo"));
}

#[test]
fn decimal_foundations_check_ranges_and_preserve_output_on_failure() {
    native(include_str!("stdlib/text_fmt_foundation.dodo"));
}

#[test]
fn owned_views_are_unsafe_to_construct_and_require_matching_access() {
    rejects(
        "package app\nimport \"core/ptr\"\nfn main(){ x:=1\np:=ptr.from_ref(&x)\ny:=ptr.borrow(p,&x)\ncore.drop(y) }",
        "requires an explicit unsafe block",
    );
    rejects(
        "package app\nimport \"core/ptr\"\nfn main(){ x:=1\np:=ptr.from_mut(&mut x)\nunsafe { y:=ptr.borrow_mut(p,&x)\ncore.drop(y) } }",
        "exclusive owner borrow",
    );
    rejects(
        "package app\nimport \"core/ptr\"\nfn main(){ x:=1\np:=ptr.from_ref(&x)\nunsafe { y:=ptr.borrow_mut(p,&mut x)\ncore.drop(y) } }",
        "mutable raw pointer",
    );
    rejects(
        "package app\nimport \"core/ptr\"\nfn main(){ x:=1\np:=ptr.from_ref(&x)\nunsafe { y:=ptr.borrow(p,x)\ncore.drop(y) } }",
        "checked owner reference",
    );
}

#[test]
fn views_cannot_escape_owner_or_erase_underlying_dependencies() {
    rejects(
        "package app\nimport \"core/ptr\"\nfn escape()->&i32 from(static) { x:=1i32\nunsafe { return ptr.borrow(ptr.from_ref(&x),&x) } }",
        "borrow",
    );
    rejects(
        "package app\nimport \"alloc/shared_arena\"\nimport \"alloc/shared_box\"\nimport \"alloc/error\"\nfn escape()->shared_box.Box<i32>!error.AllocError from(static) { bytes:=[0u8;128]\na:=shared_arena.SharedArena.new(&mut bytes)?\nreturn shared_box.new(a.handle(),1i32) }",
        "borrow",
    );
    let before = "package app\nimport \"alloc/shared_arena\"\nimport \"alloc/shared_box\"\nimport \"alloc/error\"\nfn test()->void!error.AllocError { bytes:=[0u8;128]\na:=shared_arena.SharedArena.new(&mut bytes)?\nb:=shared_box.new(a.handle(),1i32)?\n";
    for operation in ["unsafe { a.reset() }", "core.drop(a)", "bytes[0]=1"] {
        rejects(
            &format!("{before}{operation}\ncore.drop(b)\nreturn ok() }}"),
            "borrow",
        );
    }
}

#[test]
fn live_views_prevent_replacement_removal_and_destruction() {
    let before = "package app\nimport \"alloc/shared_arena\"\nimport \"alloc/shared_box\"\nimport \"alloc/error\"\nfn test()->i32!error.AllocError { bytes:=[0u8;128]\na:=shared_arena.SharedArena.new(&mut bytes)?\nb:=shared_box.new(a.handle(),1i32)?\nr:=b.get()\n";
    for operation in [
        "core.drop(b)",
        "old:=b.replace(2)",
        "r2:=b.get_mut()\n*r2=3",
    ] {
        rejects(&format!("{before}{operation}\nreturn ok(*r) }}"), "borrow");
    }
    rejects(
        "package app\nimport \"core/ptr\"\nfn main()->i32 { x:=1i32\np:=ptr.from_mut(&mut x)\nunsafe { a:=ptr.borrow_mut(p,&mut x)\nb:=ptr.borrow_mut(p,&mut x)\n*a=2\nreturn *b } }",
        "borrow",
    );
}

#[test]
fn opaque_views_reject_nested_reference_and_result_payloads() {
    for ty in ["&i32", "Option<&i32>", "Result<i32,i32>", "Wrapped"] {
        rejects(
            &format!(
                "package app\nimport \"core/ptr\"\nstruct Wrapped {{ field: Option<&i32> }}\nunsafe fn test(p:*const {ty},owner:&u8) {{ unsafe {{ r:=ptr.borrow(p,owner)\ncore.drop(r) }} }}"
            ),
            "checked-borrow elements or unhandled Results",
        );
    }
}

#[test]
fn shared_dependencies_do_not_become_exclusive_when_mutating_owned_fields() {
    native(
        "package app\nstruct Wrapper { source:&i32, own:i32 }\nfn set(w:&mut Wrapper){w.own=5}\nfn main()->i32 { source:=7i32\na:=Wrapper{source:&source,own:0}\nb:=Wrapper{source:&source,own:1}\nset(&mut a)\nreturn a.own + *b.source - 12 }",
    );
    rejects(
        "package app\nstruct Wrapper { source:&i32, own:i32 }\nfn main()->i32 { source:=7i32\na:=Wrapper{source:&source,own:0}\nr:=&mut a\nsource=8\nreturn *r.source }",
        "borrow",
    );
    rejects(
        "package app\nstruct Wrapper { source:&i32, own:i32 }\nfn main()->i32 { source:=7i32\na:=Wrapper{source:&source,own:0}\nr:=&mut a\nr2:=&a\nr.own=5\nreturn r2.own }",
        "borrow",
    );
}

#[test]
fn exclusive_dependencies_still_prevent_aliases() {
    rejects(
        "package app\nstruct Wrapper { source:&mut i32 }\nfn main()->i32 { source:=7i32\na:=Wrapper{source:&mut source}\nr:=&mut a\nsource=8\nreturn *r.source }",
        "borrow",
    );
    rejects(
        "package app\nstruct Wrapper { source:&mut i32 }\nfn test(w:Wrapper){ r:=&mut w\np:=&mut *r.source\nq:=&mut *r.source\n*p=1\n*q=2 }",
        "borrow",
    );
}

#[test]
fn shared_aggregate_routes_cannot_upgrade_stored_exclusive_references() {
    for (body, error) in [
        ("fn set(w:&W){ *w.p=2 }", "shared"),
        (
            "fn get(w:&W)->&mut i32 from(w){ return &mut *w.p }",
            "shared",
        ),
        (
            "fn set(p:&mut i32){*p=2}\nfn access(w:&W){set(w.p)}",
            "expected `&mut i32`, found `&i32`",
        ),
        ("fn set(w:&W){ p:=w.p\n*p=2 }", "shared"),
    ] {
        rejects(
            &format!("package app\nstruct W {{ p:&mut i32 }}\n{body}"),
            error,
        );
    }
    rejects(
        "package app\nstruct W{p:&mut[i32]}\nfn set(w:&W){w.p[0]=2}",
        "shared",
    );
    rejects(
        "package app\nstruct W{p:&mut[i32]}\nfn get(w:&W)->&mut[i32] from(w){return &mut w.p[0..1]}",
        "shared",
    );
    // Binding immutability itself does not weaken a stored exclusive reference.
    native(
        "package app\nstruct W{p:&mut i32}\nfn main()->i32{x:=1i32\nlet w=W{p:&mut x}\n*w.p=3\nreturn x-3}",
    );
    native(
        "package app\nimport \"core/ptr\"\nstruct W{p:*mut i32}\nfn set(w:&W){unsafe{*w.p=3}}\nfn main()->i32{x:=1i32\nw:=W{p:ptr.from_mut(&mut x)}\nset(&w)\nreturn x-3}",
    );
}

#[test]
fn generic_contracts_keep_actual_dependencies_and_check_source_names() {
    native(
        "package app\nfn identity<T>(x:T)->T from(x){return x}\nfn main()->i32 {x:=identity(3i32)\nreturn x-3}",
    );
    rejects(
        "package app\nfn identity<T>(x:T)->T from(missing){return x}\nfn main()->i32 {return identity(3i32)}",
        "not a parameter",
    );
    rejects(
        "package app\nfn identity<T>(x:T)->T from(x){return x}\nfn escape()->&i32 from(static){x:=3i32\nreturn identity(&x)}",
        "borrow",
    );
    rejects(
        "package app\nfn identity(x:i32)->i32 from(x){return x}",
        "borrow-carrying return type",
    );
}
