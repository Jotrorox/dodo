//! The pure-Dodo Brotli decoder (std/compress/brotli): the committed
//! fixtures of tests/compress/brotli (qualities 0 to 11, window sizes 10 to
//! 24, dictionary-heavy text, binary, random and repetitive data,
//! multi-meta-block and hand-written streams, truncation and corruption),
//! run unoptimized and optimized; a live differential test against the
//! brotli command-line tool when it is installed; and the generated static
//! dictionary checked against the length and CRC-32 that RFC 7932 states.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn brotli_decoder_matches_fixtures() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for optimization in ["0", "3"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["test", "tests/compress/brotli", "-O", optimization])
            .current_dir(&root)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("0 failed"),
            "dodo test tests/compress/brotli -O{optimization}: {}\n{}\n{}",
            output.status,
            stdout,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Decodes the body of a Dodo byte-string literal.
fn unescape(literal: &str) -> Vec<u8> {
    let bytes = literal.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        match bytes[i + 1] {
            b'x' => {
                let hex = std::str::from_utf8(&bytes[i + 2..i + 4]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 4;
            }
            other => {
                out.push(other);
                i += 2;
            }
        }
    }
    out
}

#[test]
fn generated_dictionary_matches_rfc_7932() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tables = std::fs::read_to_string(root.join("stdlib/std/compress/brotli/tables.dodo")).unwrap();
    let line = tables
        .lines()
        .find(|line| line.starts_with("const DICTIONARY: &[u8] = b\""))
        .expect("DICTIONARY literal");
    let body = &line["const DICTIONARY: &[u8] = b\"".len()..line.len() - 1];
    let dictionary = unescape(body);
    assert_eq!(dictionary.len(), 122_784);
    assert_eq!(crc32(&dictionary), 0x5136_cb04);
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn brotli(args: &[&str], input: &[u8]) -> Option<(bool, Vec<u8>)> {
    let mut child = Command::new("brotli")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take().unwrap();
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    Some((output.status.success(), output.stdout))
}

/// Runs the oracle in one-shot (0) or streaming (2) mode; returns its exit
/// code and output.
fn oracle(executable: &PathBuf, mode: u8, stream: &[u8]) -> (i32, Vec<u8>) {
    let mut input = vec![mode, 0, 0, 0, 0];
    input.extend_from_slice(stream);
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    (output.status.code().unwrap_or(-1), output.stdout)
}

#[test]
fn brotli_decoder_matches_brotli_tool() {
    if brotli(&["--version"], b"").is_none() {
        eprintln!("skipping: brotli tool not installed");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let executable = std::env::temp_dir().join(format!(
        "dodo-brotli-oracle-{}{}",
        std::process::id(),
        std::env::consts::EXE_SUFFIX
    ));
    let build = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(root.join("tests/stdlib/brotli_oracle.dodo"))
        .args(["-O", "0", "-o"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let words: Vec<&[u8]> = vec![
        b"the ", b"of ", b"and ", b"brotli ", b"window ", b"The ", b"dictionary, ",
        b"compression. ", b"<a href=\"", b"\">", b"information ", b"\n",
    ];
    for case in 0..48 {
        let size = rng.below(40_000) as usize;
        let data: Vec<u8> = match case % 4 {
            0 => {
                let alphabet = 1 + rng.below(256);
                (0..size).map(|_| rng.below(alphabet) as u8).collect()
            }
            1 => {
                let mut text = Vec::new();
                while text.len() < size {
                    text.extend_from_slice(words[rng.below(words.len() as u64) as usize]);
                }
                text
            }
            2 => (0..size).map(|i| ((i * i) >> 7) as u8 ^ (i as u8)).collect(),
            _ => {
                let chunk: Vec<u8> = (0..1 + rng.below(300)).map(|_| rng.next() as u8).collect();
                chunk.iter().cycle().take(size).copied().collect()
            }
        };
        let quality = (case % 12).to_string();
        let window = (10 + rng.below(15)).to_string();
        let (ok, stream) = brotli(&["-c", "-q", &quality, "-w", &window], &data).unwrap();
        assert!(ok);
        for mode in [0u8, 2] {
            let (code, output) = oracle(&executable, mode, &stream);
            assert_eq!(code, 0, "case {case} q{quality} w{window} mode {mode}");
            assert!(output == data, "case {case} q{quality} w{window} mode {mode}: output differs");
        }
        // Corrupted copies: accepted exactly when the tool accepts them,
        // with the same output.
        for _ in 0..6 {
            if stream.is_empty() {
                break;
            }
            let mut broken = stream.clone();
            let at = rng.below(broken.len() as u64) as usize;
            broken[at] ^= 1 << rng.below(8);
            let (accepted, expected) = brotli(&["-d", "-c"], &broken).unwrap();
            let (code, output) = oracle(&executable, 0, &broken);
            assert!(
                (10..=15).contains(&code) || code == 0,
                "case {case}: oracle exit {code}"
            );
            assert_eq!(code == 0, accepted, "case {case}: verdict differs at byte {at}");
            if accepted {
                assert!(output == expected, "case {case}: output differs at byte {at}");
            }
        }
    }
    std::fs::remove_file(executable).unwrap();
}
