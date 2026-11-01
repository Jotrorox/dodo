//! Error paths at the LLVM C API boundary must leave owners usable and droppable.
use dodoc::codegen::{self, Context, FileType, Options, PanicStrategy};
use dodoc::{package::Source, parser, sema};
use std::path::Path;

#[test]
fn returned_zero_arrays_remain_compact_during_optimization() {
    let mut program = parser::parse(
        "package arrays\n\
         pub struct Buffer { pub data: [8192]u64 }\n\
         pub fn zeroes() -> [8192]u64 { return [0u64; 8192] }\n\
         pub fn nested() -> Buffer { return Buffer { data: [0u64; 8192] } }\n",
    )
    .unwrap();
    sema::check(&mut program).unwrap();
    for optimization in 0..=3 {
        let context = Context::create();
        let options = Options {
            optimization,
            ..Default::default()
        };
        let generated = codegen::generate(&context, &program, &options).unwrap();
        let ir = generated.module.print_to_string();
        // Large results return through memory, zeroed by memset.
        assert!(ir.contains("@llvm.memset"), "{ir}");
        assert!(!ir.contains("store [8192 x i64]"), "{ir}");
        assert!(
            ir.len() < 4096,
            "zero array initialization expanded: {} bytes",
            ir.len()
        );
    }
}

#[test]
fn invalid_target_options_return_errors() {
    for options in [
        Options {
            target: Some("not-a-real-target".into()),
            ..Default::default()
        },
        Options {
            target: Some("x86_64\0-linux".into()),
            ..Default::default()
        },
        Options {
            cpu: Some("generic\0cpu".into()),
            ..Default::default()
        },
        Options {
            features: "+sse2\0feature".into(),
            ..Default::default()
        },
    ] {
        assert!(
            !codegen::pointer_bits(&options)
                .unwrap_err()
                .to_string()
                .is_empty()
        );
    }
}

#[test]
fn debug_codegen_error_releases_builders_before_the_module() {
    let text = "package cleanup\nfn main() -> i32 { return 1 / 0 }\n";
    let mut program = parser::parse(text).unwrap();
    sema::check(&mut program).unwrap();
    let context = Context::create();
    let mut options = Options {
        debug: true,
        panic: PanicStrategy::Hook("bad hook".into()),
        sources: vec![Source {
            path: "cleanup.dodo".into(),
            text: text.into(),
            start: 0,
        }],
        ..Default::default()
    };
    let err = codegen::generate(&context, &program, &options)
        .err()
        .unwrap();
    assert!(
        err.to_string()
            .contains("panic hook must be a C symbol name")
    );

    // Reuse the context after abandoning a partially constructed debug module.
    options.panic = PanicStrategy::Trap;
    let generated = codegen::generate(&context, &program, &options).unwrap();
    generated.module.verify().unwrap();
    assert!(
        generated
            .module
            .print_to_string()
            .contains("!DICompileUnit")
    );
}

#[test]
fn failed_output_does_not_invalidate_the_module_or_target_machine() {
    let mut program = parser::parse("package output\npub fn answer() -> i32 { 42 }\n").unwrap();
    sema::check(&mut program).unwrap();
    let context = Context::create();
    let generated = codegen::generate(&context, &program, &Options::default()).unwrap();
    // Cargo.toml is a file, so it cannot serve as the output directory.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml/output");
    assert!(generated.module.print_to_file(&path).is_err());
    assert!(!generated.module.write_bitcode_to_path(&path));
    assert!(
        generated
            .machine
            .write_to_file(&generated.module, FileType::Object, &path)
            .is_err()
    );
    let object = generated
        .machine
        .write_to_memory_buffer(&generated.module, FileType::Object)
        .unwrap();
    assert!(!object.as_slice().is_empty());
    assert!(
        object
            .sections()
            .unwrap()
            .iter()
            .any(|section| section.size > 0)
    );
}

#[test]
fn objects_never_reference_unwinding_personality_routines() {
    let text = "package add\npub fn add(a: u32, b: u32) -> u32 { return a + b }\n";
    let mut program = parser::parse(text).unwrap();
    sema::check(&mut program).unwrap();
    let context = Context::create();
    let object = |target: &str, optimization, debug| {
        let options = Options {
            target: Some(target.into()),
            optimization,
            debug,
            panic: PanicStrategy::Hook("board_panic".into()),
            sources: vec![Source {
                path: "add.dodo".into(),
                text: text.into(),
                start: 0,
            }],
            ..Default::default()
        };
        let generated = codegen::generate(&context, &program, &options).unwrap();
        generated
            .machine
            .write_to_memory_buffer(&generated.module, FileType::Object)
            .unwrap()
            .sections()
            .unwrap()
    };
    // Dodo traps instead of unwinding, so Arm EHABI index entries must not pull
    // in libgcc's personality routines when firmware links with -nostdlib.
    for target in ["thumbv6m-none-eabi", "thumbv7em-none-eabihf"] {
        for optimization in [0, 2] {
            for debug in [false, true] {
                let sections = object(target, optimization, debug);
                let symbols: Vec<_> = sections
                    .iter()
                    .flat_map(|section| &section.relocation_symbols)
                    .collect();
                assert!(
                    symbols.iter().any(|symbol| *symbol == "board_panic"),
                    "{target} -O{optimization}: {symbols:?}"
                );
                assert!(
                    !symbols
                        .iter()
                        .any(|symbol| symbol.starts_with("__aeabi_unwind_cpp_pr")),
                    "{target} -O{optimization} debug={debug}: {symbols:?}"
                );
            }
        }
    }
    // Hosted targets keep unwind tables for debuggers and crash backtraces.
    let sections = object("x86_64-unknown-linux-gnu", 2, false);
    assert!(
        sections.iter().any(|section| section.name == ".eh_frame"),
        "{:?}",
        sections
            .iter()
            .map(|section| &section.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn result_and_enum_payloads_are_stored_as_integer_words() {
    // LLVM returns every array element of a first-class aggregate in its own
    // register. Byte storage made an `i64!E` result need nine values, which
    // demoted the return to memory and reassembled the payload byte by byte.
    let mut program = parser::parse(
        "package payloads\n\
         pub struct Throw { pub code: i32 }\n\
         pub enum Mixed { Byte(u8), Pair(u16, u32), Wide(u64) }\n\
         pub fn wide(x: i64) -> i64!Throw { if x < 0 { return err(Throw { code: 1 }) } return ok(x) }\n\
         pub fn narrow(x: i32) -> i32!u8 { if x < 0 { return err(1u8) } return ok(x) }\n\
         pub fn bytes(x: u8) -> u8!u8 { if x == 0 { return err(1u8) } return ok(x) }\n\
         pub fn mixed(x: u64) -> Mixed { return Mixed.Wide(x) }\n",
    )
    .unwrap();
    sema::check(&mut program).unwrap();
    // 32-bit targets cap the word at a pointer.
    for (target, words) in [(None, "1 x i64"), (Some("thumbv6m-none-eabi"), "2 x i32")] {
        let context = Context::create();
        let options = Options {
            target: target.map(Into::into),
            ..Default::default()
        };
        let generated = codegen::generate(&context, &program, &options).unwrap();
        let ir = generated.module.print_to_string();
        for expected in [
            format!("{{ i1, {{ [0 x i64], [{words}] }} }} @dodo.payloads.wide("),
            "{ i1, { [0 x i32], [1 x i32] } } @dodo.payloads.narrow(".to_string(),
            "{ i1, { [0 x i8], [1 x i8] } } @dodo.payloads.bytes(".to_string(),
            format!("%Mixed = type {{ i32, {{ [0 x {{ i64 }}], [{words}] }} }}"),
        ] {
            assert!(
                ir.contains(&expected),
                "{target:?}: missing `{expected}` in\n{ir}"
            );
        }
    }
}
