//! Hosted convenience APIs against independent local peers at O0 and O3.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn hosted_http_and_https_interoperate_with_independent_peers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .arg(root.join("scripts/test_hosted_http.py"))
        .arg("--compiler")
        .arg(env!("CARGO_BIN_EXE_dodo"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn hosted_client_keeps_workspace_and_header_views_borrowed() {
    let scratch =
        std::env::temp_dir().join(format!("dodo-http-hosted-borrows-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    for (name, body) in [
        (
            "workspace",
            "workspace[0] = 1\n match client.header(b\"X\", 0) { some(_) => {} none => {} }",
        ),
        (
            "headers",
            "view := client.header(b\"X\", 0)\n data := [0u8; 8]\n match client.get(b\"http://localhost/\", &mut data) { ok(_) => {} err(_) => {} }\n match view { some(field) => { return field.value.len as i32 } none => {} }",
        ),
    ] {
        let path = scratch.join(format!("{name}.dodo"));
        std::fs::write(&path, format!("package borrow_check\nimport \"std/http/hosted\"\nfn main() -> i32 {{\n workspace := [0u8; hosted.WORKSPACE_BYTES]\n client: hosted.Client\n match hosted.Client.new(&mut workspace, hosted.Config.defaults()) {{ ok(value) => {{ client = value }} err(_) => {{ return 1 }} }}\n{body}\n return 0\n}}\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(path)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {name} borrow escape");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("borrow"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::remove_dir_all(scratch).unwrap();
}
