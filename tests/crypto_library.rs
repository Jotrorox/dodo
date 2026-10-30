//! Published crypto vectors, OS entropy, and RSA interoperability with an
//! independent OpenSSL command-line peer. Keys are generated per run.
use dodoc::package;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "dodo-crypto-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output, context: &str) -> Output {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn build(source: &Path, executable: &Path, level: &str) {
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .args(["-O", level, "-o"])
            .arg(executable)
            .output()
            .unwrap(),
        "compile crypto fixture",
    );
}
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/crypto/{name}"))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}
fn openssl(directory: &Path, arguments: &[&str], context: &str) -> Output {
    success(
        Command::new("openssl")
            .args(arguments)
            .current_dir(directory)
            .output()
            .unwrap(),
        context,
    )
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn published_vectors_entropy_and_rsa_round_trips() {
    let workspace = Workspace::new();
    for level in ["0", "3"] {
        let executable = workspace.0.join(format!("checks-{level}"));
        build(&fixture("checks.dodo"), &executable, level);
        success(
            Command::new(&executable).output().unwrap(),
            "run crypto vectors",
        );
    }
}

/// Our DER parses as an RSA SubjectPublicKeyInfo in OpenSSL's CLI, and
/// PKCS#1 v1.5 ciphertexts cross between the two in both directions.
#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rsa_pkcs1_and_der_interoperate_with_openssl_cli() {
    let workspace = Workspace::new();
    let directory = &workspace.0;
    openssl(
        directory,
        &[
            "genpkey",
            "-algorithm",
            "RSA",
            "-pkeyopt",
            "rsa_keygen_bits:2048",
            "-out",
            "peer.pem",
        ],
        "generate peer key",
    );
    openssl(
        directory,
        &[
            "pkey", "-in", "peer.pem", "-pubout", "-outform", "DER", "-out", "peer.der",
        ],
        "export peer public key",
    );
    let peer_der = fs::read(directory.join("peer.der")).unwrap();
    for level in ["0", "3"] {
        let executable = directory.join(format!("interop-{level}"));
        build(&fixture("interop.dodo"), &executable, level);
        let mut child = Command::new(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut next = || lines.next().expect("fixture line").unwrap();

        let ours = unhex(&next());
        fs::write(directory.join("ours.der"), &ours).unwrap();
        let text = openssl(
            directory,
            &[
                "pkey", "-pubin", "-inform", "DER", "-in", "ours.der", "-noout", "-text",
            ],
            "parse Dodo public key",
        );
        let text = String::from_utf8_lossy(&text.stdout);
        assert!(text.contains("Public-Key: (1024 bit)"), "{text}");
        assert!(text.contains("Exponent: 65537"), "{text}");

        let secret: Vec<u8> = (0u8..16).map(|i| i.wrapping_mul(37) ^ 0x5a).collect();
        fs::write(directory.join("secret.bin"), &secret).unwrap();
        openssl(
            directory,
            &[
                "pkeyutl",
                "-encrypt",
                "-pubin",
                "-keyform",
                "DER",
                "-inkey",
                "ours.der",
                "-pkeyopt",
                "rsa_padding_mode:pkcs1",
                "-in",
                "secret.bin",
                "-out",
                "secret.enc",
            ],
            "encrypt to Dodo key",
        );
        let ciphertext = fs::read(directory.join("secret.enc")).unwrap();
        assert_eq!(ciphertext.len(), 128);
        writeln!(input, "{}", hex(&ciphertext)).unwrap();
        assert_eq!(unhex(&next()), secret, "Dodo PKCS#1 v1.5 decryption");

        writeln!(input, "{}", hex(&peer_der)).unwrap();
        let ciphertext = unhex(&next());
        assert_eq!(ciphertext.len(), 256);
        fs::write(directory.join("message.enc"), &ciphertext).unwrap();
        // Implicit rejection is off here, so wrong padding fails loudly
        // instead of decrypting to a pseudorandom message.
        let plain = openssl(
            directory,
            &[
                "pkeyutl",
                "-decrypt",
                "-inkey",
                "peer.pem",
                "-pkeyopt",
                "rsa_padding_mode:pkcs1",
                "-pkeyopt",
                "rsa_pkcs1_implicit_rejection:0",
                "-in",
                "message.enc",
            ],
            "decrypt Dodo ciphertext",
        );
        assert_eq!(plain.stdout, b"dodo interop secret");

        drop(input);
        success(child.wait_with_output().unwrap(), "finish interop fixture");
    }
}

#[test]
fn crypto_contract_is_portable_and_backends_link_independently() {
    let workspace = Workspace::new();
    let source = workspace.0.join("portable.dodo");
    fs::write(&source, "package portable\nimport \"std/crypto\"\nfn same(a: &[u8], b: &[u8]) -> bool { return crypto.equal(a, b) }\nfn main() {}\n").unwrap();
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        let loaded = package::load_for_target(&source, target).unwrap();
        assert!(package::native_sources(&loaded).is_empty(), "{target}");
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["--target", target, "--emit", "obj", "-o"])
                .arg(workspace.0.join("portable.o"))
                .output()
                .unwrap(),
            "cross compile portable crypto contract",
        );
    }
    // Entropy needs no OpenSSL, and libcrypto does not drag in the entropy
    // boundary or libssl's TLS runtime.
    for (import, expected) in [
        ("std/crypto/random", "std/crypto/runtime.c"),
        ("std/crypto/native", "std/crypto/runtime.c"),
        ("std/crypto/openssl", "std/crypto/openssl/runtime.c"),
        ("std/crypto/openssl/native", "std/crypto/openssl/runtime.c"),
    ] {
        fs::write(
            &source,
            format!("package backend\nimport \"{import}\"\nfn main() {{}}\n"),
        )
        .unwrap();
        for target in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ] {
            let loaded = package::load_for_target(&source, target).unwrap();
            let names: Vec<_> = package::native_sources(&loaded)
                .iter()
                .map(|(name, _)| *name)
                .collect();
            assert_eq!(names, [expected], "{import} for {target}");
        }
        assert!(
            package::load_for_target(&source, "thumbv6m-none-eabi")
                .unwrap_err()
                .contains("unsupported"),
            "{import}"
        );
    }
    fs::write(
        &source,
        "package mismatch\nimport \"std/crypto/openssl/windows\"\nfn main() {}\n",
    )
    .unwrap();
    let error = package::load_for_target(&source, "x86_64-unknown-linux-gnu").unwrap_err();
    assert!(error.contains("std/crypto/openssl/native"), "{error}");
}
