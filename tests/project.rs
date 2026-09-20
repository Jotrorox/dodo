use dodoc::project::{self, Manifest, Panic, Purpose, Request, Settings};
use std::ffi::OsString;
use std::path::Path;
fn manifest(s: &str) -> Manifest {
    Manifest::parse(s, Path::new("/work/dodo.toml")).unwrap()
}
fn resolve(
    m: Option<&Manifest>,
    target: Option<&str>,
    profile: Option<&str>,
    purpose: Purpose,
    settings: &Settings,
    args: &[OsString],
    clear: bool,
) -> project::Resolved {
    project::resolve(Request {
        manifest: m,
        target,
        profile,
        purpose,
        overrides: settings,
        link_args: args,
        clear_link_args: clear,
        env_linker: None,
        cwd: Path::new("/caller"),
        host: "x86_64-unknown-linux-gnu",
    })
    .unwrap()
}
#[test]
fn settings_only_manifest_and_strict_schema() {
    let m = manifest("schema=1\n[build]\nopt-level=2");
    assert!(m.targets.contains_key("app"));
    for s in [
        "",
        "schema=2",
        "schema='1'",
        "schema=1\nwat=true",
        "schema=1\n[build]\nopt-level=9",
        "schema=1\n[build]\ndebug='true'",
        "schema=1\n[targets.x]\nemit='obj'",
        "schema=1\n[profiles.release]\ntriple='bad'",
        "schema=1\n[test]\nfilter=['hidden']",
        "schema=1\n[test.build]\ntriple='wasm32-unknown-unknown'",
        "schema=1\n[build]\npanic='auto'\npanic-hook='board'",
        "schema=1\n[test]\ntimeout=nan",
    ] {
        let error = Manifest::parse(s, Path::new("dodo.toml")).unwrap_err();
        assert!(error.starts_with("dodo.toml:"), "{error}");
    }
    manifest("schema=1\n[tool.example]\nanything={when=2024-01-01, values=[1,'yes',true]}");
}
#[test]
fn profiles_cli_and_whole_panic_policy_precedence() {
    let m = manifest(
        "schema=1\n[build]\nopt-level=1\ndebug=true\npanic-hook='board'\n[targets.app]\nentry='main.dodo'\nopt-level=2\n[profiles.release]\ndebug=true",
    );
    let r = resolve(
        Some(&m),
        None,
        Some("release"),
        Purpose::Build,
        &Settings::default(),
        &[],
        false,
    );
    assert_eq!(r.settings.opt_level, Some(3));
    assert_eq!(r.settings.debug, Some(true));
    assert_eq!(r.origins["debug"], "profiles.release");
    let r = resolve(
        Some(&m),
        None,
        Some("release"),
        Purpose::Build,
        &Settings {
            opt_level: Some(0),
            debug: Some(false),
            panic: Some(Panic::Trap),
            ..Default::default()
        },
        &[],
        false,
    );
    assert_eq!(r.settings.opt_level, Some(0));
    assert_eq!(r.settings.debug, Some(false));
    assert_eq!(r.settings.panic, Some(Panic::Trap));
    assert_eq!(r.origins["panic"], "CLI");
}
#[test]
fn host_tests_do_not_inherit_firmware_settings() {
    let m = manifest(
        "schema=1\n[build]\ntriple='thumbv7em-none-eabi'\npanic-hook='board'\nlinker='arm-linker'\n[targets.firmware]\nentry='board.dodo'\nemit='obj'\n[test]\nroot='tests'\ntimeout=1.5\n[test.build]\ndebug=true",
    );
    let r = resolve(
        Some(&m),
        None,
        None,
        Purpose::Test,
        &Settings::default(),
        &[],
        false,
    );
    assert_eq!(
        r.settings.triple.as_deref(),
        Some("x86_64-unknown-linux-gnu")
    );
    assert_eq!(r.settings.linker, Some("cc".into()));
    assert_eq!(r.settings.panic, Some(Panic::Auto));
    assert_eq!(r.entry.as_deref(), Some(Path::new("/work/tests")));
    assert_eq!(r.timeout.as_secs_f64(), 1.5);
}
#[test]
fn linker_arguments_replace_append_and_clear() {
    let m = manifest(
        "schema=1\n[build]\nlink-args=['-lbase']\n[targets.app]\nentry='main.dodo'\nlink-args=['-lapp']",
    );
    let r = resolve(
        Some(&m),
        None,
        None,
        Purpose::Build,
        &Settings::default(),
        &["-s".into()],
        false,
    );
    assert_eq!(
        r.settings.link_args.unwrap(),
        vec![OsString::from("-lapp"), "-s".into()]
    );
    let r = resolve(
        Some(&m),
        None,
        None,
        Purpose::Build,
        &Settings::default(),
        &["-s".into()],
        true,
    );
    assert_eq!(r.settings.link_args.unwrap(), vec![OsString::from("-s")]);
}
#[test]
fn target_selection_and_case_collisions() {
    let m = manifest("schema=1\n[targets.alpha]\nentry='a.dodo'\n[targets.beta]\nentry='b.dodo'");
    assert!(m.select(None).is_err());
    assert_eq!(m.select(Some("beta")).unwrap().0, "beta");
    assert!(m.select(Some("alpa")).unwrap_err().contains("alpha"));
    for s in [
        "schema=1\n[targets.con]\nentry='a.dodo'",
        "schema=1\n[targets.App]\nentry='a.dodo'\n[targets.app]\nentry='a.dodo'",
        "schema=1\n[project]\ndefault-target='missing'",
    ] {
        assert!(Manifest::parse(s, Path::new("dodo.toml")).is_err());
    }
}
#[test]
fn standalone_profiles_and_manifest_paths_are_resolved_without_environment_reads() {
    let r = resolve(
        None,
        None,
        Some("release"),
        Purpose::Build,
        &Settings::default(),
        &[],
        false,
    );
    assert_eq!(r.settings.opt_level, Some(3));
    assert!(r.output.is_none());
    let m = manifest(
        "schema=1\n[build]\nlinker='./tools/cc'\n[targets.web]\nentry='src/main.dodo'\n[targets.web.run]\ncwd='fixtures'\nargs=['--port','8080']",
    );
    let r = resolve(
        Some(&m),
        None,
        None,
        Purpose::Run,
        &Settings::default(),
        &[],
        false,
    );
    assert_eq!(
        r.settings.linker,
        Some(Path::new("/work").join("./tools/cc").into_os_string())
    );
    assert_eq!(r.run_cwd.as_ref().unwrap(), Path::new("/work/fixtures"));
    assert_eq!(r.run_args.len(), 2);
    assert!(dodoc::toml::parse(&r.report()).is_ok());
}
