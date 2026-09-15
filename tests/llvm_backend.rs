//! Error paths at the LLVM C API boundary must leave owners usable and droppable.
use dodoc::codegen::{self, Context, FileType, Options, PanicStrategy};
use dodoc::{package::Source, parser, sema};
use std::path::Path;

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
