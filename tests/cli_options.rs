use dodoc::cli::{self, Action, Invocation, Parsed};
use std::ffi::OsString;
fn parse(args: &[&str]) -> Result<Parsed, String> {
    cli::parse(args.iter().map(OsString::from))
}
fn invoke(args: &[&str]) -> Invocation {
    let Parsed::Invoke(i) = parse(args).unwrap() else {
        panic!()
    };
    *i
}
#[test]
fn every_command_has_specific_help_and_help_does_not_resolve_projects() {
    for (name, action) in [
        ("build", Action::Build),
        ("run", Action::Run),
        ("check", Action::Check),
        ("test", Action::Test),
        ("fmt", Action::Fmt),
        ("lsp", Action::Lsp),
    ] {
        assert!(matches!(parse(&["help",name]).unwrap(),Parsed::Help(Some(a)) if a==action));
        assert!(matches!(parse(&[name,"--help"]).unwrap(),Parsed::Help(Some(a)) if a==action));
    }
    assert!(parse(&["help", "nonsense"]).is_err());
    assert!(parse(&["biuld"]).unwrap_err().contains("build"));
    assert!(!cli::help(Some(Action::Run)).contains("--output"));
    assert!(!cli::help(Some(Action::Fmt)).contains("--linker"));
    assert!(!cli::help(Some(Action::Check)).contains("--panic"));
}
#[test]
fn equivalent_option_forms_and_last_explicit_overrides() {
    for args in [
        &["build", "--opt-level=2", "--output=a b", "-bapp"][..],
        &["build", "-O2", "-oa b", "--build-target=app"],
        &["build", "-O", "2", "-o", "a b", "-b", "app"],
    ] {
        let i = invoke(args);
        assert_eq!(i.settings.opt_level, Some(2));
        assert_eq!(i.output.unwrap().to_str(), Some("a b"));
        assert_eq!(i.build_target.as_deref(), Some("app"));
    }
    assert_eq!(
        invoke(&["test", "-g", "--no-debug", "--timeout=1.5"])
            .settings
            .debug,
        Some(false)
    );
}
#[test]
fn run_and_linker_boundaries_are_preserved() {
    assert!(invoke(&["run"]).run_args.is_none());
    assert_eq!(invoke(&["run", "--"]).run_args, Some(vec![]));
    assert_eq!(
        invoke(&["run", "--", "--help", "a b"]).run_args,
        Some(vec!["--help".into(), "a b".into()])
    );
    let i = invoke(&["build", "--link-arg", "--help", "--link-arg=-s"]);
    assert_eq!(i.link_args, vec![OsString::from("--help"), "-s".into()]);
    assert!(
        parse(&["check", "--target", "--help"])
            .unwrap_err()
            .contains("requires a value")
    );
    assert!(
        parse(&["run", "--port", "8080"])
            .unwrap_err()
            .contains("arguments must follow")
    );
}
#[test]
fn invalid_combinations_and_command_scoping() {
    for args in [
        &["build", "--release", "--profile=dev"][..],
        &["build", "--all-targets", "-bapp"],
        &["build", "--all-targets", "-ox"],
        &["build", "--no-manifest", "--manifest-path=x"],
        &["check", "--release"],
        &["test", "--target=wasm32-unknown-unknown"],
        &["test", "--list", "--print-config"],
        &["fmt", "--check", "--stdout"],
        &["build", "-q", "-v"],
        &["build", "--debug=true"],
        &["test", "--exact"],
        &["test", "--timeout=NaN"],
    ] {
        assert!(parse(args).is_err(), "{args:?}");
    }
    assert!(parse(&["test", "tests", "--manifest-path=dodo.toml"]).is_ok());
    assert!(parse(&["run", "main.dodo", "--manifest-path=dodo.toml"]).is_err());
    assert_eq!(invoke(&["check", "--linker=unused", "-g"]).notices.len(), 2);
}
#[cfg(unix)]
#[test]
fn non_utf8_attached_paths_and_process_arguments_remain_lossless() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let Parsed::Invoke(i) = cli::parse([
        "build".into(),
        OsString::from_vec(b"--output=out\xff".to_vec()),
    ])
    .unwrap() else {
        panic!()
    };
    assert_eq!(i.output.unwrap().as_os_str().as_bytes(), b"out\xff");
    let Parsed::Invoke(i) =
        cli::parse(["run".into(), "--".into(), OsString::from_vec(vec![255])]).unwrap()
    else {
        panic!()
    };
    assert_eq!(i.run_args.unwrap()[0].as_bytes(), &[255]);
}

#[test]
fn negative_cpu_features_keep_the_existing_separated_form() {
    assert_eq!(
        invoke(&["build", "--features", "-avx,+sse2"])
            .settings
            .features
            .as_deref(),
        Some("-avx,+sse2")
    );
    assert!(parse(&["completions"]).is_err());
    assert!(parse(&["completions", "unknown"]).is_err());
}
