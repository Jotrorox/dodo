//! Dead imports must not introduce code or linker dependencies, even at O0.
use dodoc::codegen::Context;
use dodoc::codegen::FileType;
use dodoc::{codegen, package, sema};
use llvm_sys::LLVMLinkage;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-reachability-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        path
    }

    fn generate<'ctx>(
        &self,
        context: &'ctx Context,
        options: &codegen::Options,
    ) -> codegen::Generated<'ctx> {
        let target = options.target.clone().unwrap_or_else(|| {
            codegen::TargetMachine::get_default_triple()
                .as_str()
                .to_string_lossy()
                .into_owned()
        });
        let mut loaded = package::load_for_target(&self.0.join("main.dodo"), &target).unwrap();
        sema::check_for_target(&mut loaded.program, codegen::pointer_bits(options).unwrap())
            .unwrap_or_else(|error| panic!("{}", loaded.render(&error)));
        let options = codegen::Options {
            sources: loaded.sources,
            ..options.clone()
        };
        codegen::generate(context, &loaded.program, &options).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unused_num_import_emits_only_the_probe_without_helper_dependencies() {
    let w = Workspace::new();
    for optimization in 0..=3 {
        for debug in [false, true] {
            let mut baseline = None;
            for import in ["", "import \"core/num\""] {
                w.file(
                    "main.dodo",
                    &format!("package probe\n{import}\nextern \"C\" fn probe() -> u32 {{ 42 }}\n"),
                );
                let context = Context::create();
                let generated = w.generate(
                    &context,
                    &codegen::Options {
                        target: Some("thumbv6m-none-eabi".into()),
                        cpu: Some("cortex-m0".into()),
                        optimization,
                        debug,
                        ..Default::default()
                    },
                );
                let definitions: Vec<_> = generated
                    .module
                    .get_functions()
                    .filter(|f| f.count_basic_blocks() != 0)
                    .map(|f| f.get_name().to_string_lossy().into_owned())
                    .collect();
                assert_eq!(definitions, ["probe"]);
                let object = generated
                    .machine
                    .write_to_memory_buffer(&generated.module, FileType::Object)
                    .unwrap();
                let mut helpers = BTreeSet::new();
                for section in object.sections().unwrap() {
                    for name in section.relocation_symbols {
                        if name.starts_with("__") {
                            assert!(
                                !["mul", "div", "mod"].iter().any(|op| name.contains(op)),
                                "unexpected arithmetic dependency: {name}"
                            );
                            helpers.insert(name);
                        }
                    }
                }
                // Debug/unoptimized objects can contain ARM unwind references;
                // an unused import must not add any helpers to the baseline.
                if let Some(baseline) = &baseline {
                    assert_eq!(&helpers, baseline);
                } else {
                    baseline = Some(helpers);
                }
            }
        }
    }
}

#[test]
fn root_apis_and_c_definitions_remain_exports_while_dead_imports_disappear() {
    let w = Workspace::new();
    w.file(
        "dependency.dodo",
        r#"package dependency
unsafe extern "C" fn missing() -> i32
fn helper() -> i32 { 42 }
pub fn answer() -> i32 { helper() }
pub fn unused() -> i32 { unsafe { missing() } }
pub fn cycle_a() -> i32 { cycle_b() }
fn cycle_b() -> i32 { cycle_a() }
extern "C" fn imported_export() -> i32 { helper() }
pub struct Item {
    pub fn unused_method(&self) -> i32 { unused() }
}
"#,
    );
    w.file(
        "main.dodo",
        r#"package api
import "dependency"
// A root type can share the imported package's name. Origin, not spelling,
// determines whether dependency.answer is an exported root method.
pub struct dependency {}
pub fn answer() -> i32 { dependency.answer() }
pub struct Item {
    pub fn value(&self) -> i32 { dependency.answer() }
    pub fn associated() -> i32 { dependency.answer() }
}
pub fn identity<T>(value: T) -> T { value }
fn unused() -> i32 { identity::<i32>(dependency.unused()) }
extern "C" fn root_export() -> i32 { dependency.answer() }
"#,
    );
    for optimization in 0..=3 {
        let context = Context::create();
        let generated = w.generate(
            &context,
            &codegen::Options {
                optimization,
                ..Default::default()
            },
        );
        for name in [
            "dodo.api.answer",
            "dodo.api.Item.value",
            "dodo.api.Item.associated",
            "root_export",
            "imported_export",
        ] {
            let function = generated.module.get_function(name).unwrap();
            assert_ne!(function.count_basic_blocks(), 0, "missing body: {name}");
            assert_eq!(
                function.get_linkage(),
                LLVMLinkage::LLVMExternalLinkage,
                "{name}"
            );
        }
        let ir = generated.module.print_to_string().to_string();
        for dead in ["unused", "cycle_a", "cycle_b", "identity$", "missing"] {
            assert!(!ir.contains(dead), "retained {dead}: {ir}");
        }
        if optimization == 0 {
            for name in ["dodo.api.dependency.answer", "dodo.api.dependency.helper"] {
                assert_eq!(
                    generated.module.get_function(name).unwrap().get_linkage(),
                    LLVMLinkage::LLVMInternalLinkage
                );
            }
        }
    }
}

#[test]
fn reachability_includes_implicit_drops_callbacks_and_generic_methods() {
    let w = Workspace::new();
    w.file(
        "dependency.dodo",
        r#"package dependency
unsafe extern "C" fn putchar(value: i32) -> i32
fn helper() { unsafe { putchar(68) } }
pub struct Resource<T> {
    pub value: T
    fn drop(&mut self) { helper() }
    pub fn unused_method(&self) { unsafe { missing() } }
}
unsafe extern "C" fn missing()
pub unsafe fn callback(data: *mut u8) { helper() }
"#,
    );
    w.file(
        "main.dodo",
        r#"package app
import "core/mem"
import "dependency"
pub fn callback_address() -> *const u8 { unsafe { mem.callback(dependency.callback) } }
fn main() { resource := dependency.Resource::<i32>{value: 42} }
"#,
    );
    let context = Context::create();
    let generated = w.generate(&context, &codegen::Options::default());
    let ir = generated.module.print_to_string().to_string();
    for live in [
        "dependency.Resource$",
        ".drop\"(",
        "dependency.callback",
        "dependency.helper",
    ] {
        assert!(ir.contains(live), "lost {live}: {ir}");
    }
    for dead in ["unused_method", "missing"] {
        assert!(!ir.contains(dead), "retained {dead}: {ir}");
    }
    for optimization in ["0", "3"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["run", "main.dodo", "-O", optimization])
            .current_dir(&w.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"D");
    }
}

#[test]
fn emitted_functions_have_separate_sections_on_elf_and_coff() {
    let w = Workspace::new();
    w.file(
        "main.dodo",
        "package sections\nextern \"C\" fn first() -> i32 { 1 }\nextern \"C\" fn second() -> i32 { 2 }\nfn main() {}\n",
    );
    for target in [
        "thumbv6m-none-eabi",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "aarch64-apple-darwin",
        "wasm32-unknown-unknown",
    ] {
        for optimization in [0, 3] {
            let context = Context::create();
            let generated = w.generate(
                &context,
                &codegen::Options {
                    target: Some(target.into()),
                    optimization,
                    entry: true,
                    ..Default::default()
                },
            );
            let object = generated
                .machine
                .write_to_memory_buffer(&generated.module, FileType::Object)
                .unwrap();
            assert!(!object.as_slice().is_empty());
            if target.contains("apple") || target.starts_with("wasm") {
                continue;
            }
            let names: Vec<_> = object
                .sections()
                .unwrap()
                .into_iter()
                .filter(|section| section.size > 0)
                .map(|section| section.name)
                .collect();
            let prefix = if target.contains("windows") {
                ".text$"
            } else {
                ".text."
            };
            for function in ["first", "second", "main", "dodo.sections.main"] {
                assert!(
                    names.contains(&format!("{prefix}{function}")),
                    "{target}: {names:?}"
                );
            }
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn linker_can_discard_an_unused_export_with_unresolved_dependencies() {
    let w = Workspace::new();
    w.file(
        "main.dodo",
        "package sections\nunsafe extern \"C\" fn missing() -> i32\nextern \"C\" fn retained() -> i32 { 42 }\nextern \"C\" fn discarded() -> i32 { unsafe { missing() } }\n",
    );
    let driver = w.file(
        "driver.c",
        "int retained(void); int main(void) { return retained(); }\n",
    );
    for optimization in [0, 3] {
        let context = Context::create();
        let generated = w.generate(
            &context,
            &codegen::Options {
                optimization,
                ..Default::default()
            },
        );
        assert!(generated.module.get_function("discarded").is_some());
        let object = w.0.join("api.o");
        generated
            .machine
            .write_to_file(&generated.module, FileType::Object, &object)
            .unwrap();
        let executable = w.0.join("driver");
        let output = Command::new("cc")
            .arg(&driver)
            .arg(&object)
            .args(["-Wl,--gc-sections", "-o"])
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(Command::new(&executable).status().unwrap().code(), Some(42));
    }
}
