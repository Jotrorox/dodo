//! Deterministic fuzz campaigns for the pure-Dodo image library
//! (stdlib/std/image). The Dodo harness in fuzz/image is built once, then
//! every target receives batches of seeded inputs: the tracked seed corpora
//! and regression inputs in fuzz/corpus/image_<target>, the image fixtures
//! of tests/, structure-aware generated files (PNG/APNG with valid CRCs and
//! zlib streams, BMP, QOI, PPM headers) and mutations of all of these (bit
//! flips, interesting values, truncation at every chunk/box boundary,
//! chunk/box length corruption with PNG CRC fix-up, chunk duplication and
//! removal, splicing). A trap, a hang or a scratch request that is not
//! justified by the image size fails the test, which prints the target and
//! seed and writes the input to a file for replay with the harness.
//!
//! Environment (see fuzz/README.md):
//!   IMAGE_FUZZ_SEED     campaign seed (default 0)
//!   IMAGE_FUZZ_CASES    generated cases per target (default per target)
//!   IMAGE_FUZZ_SECONDS  run each target this long instead
//!   IMAGE_FUZZ_TARGET   comma-separated targets (default all)
//!   IMAGE_FUZZ_INPUT    replay one file (or every file of a directory)
//!   IMAGE_FUZZ_OPT      harness optimization level (default 1)
//!   IMAGE_FUZZ_TIMEOUT  seconds one input may take (default 30)
//!   IMAGE_FUZZ_JOBS     harness processes run at once (default up to 4)
//!   IMAGE_FUZZ_DUMP     write the campaign's inputs to this directory
//!                       instead of running them
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const TARGETS: &[&str] = &[
    "png",
    "bmp",
    "qoi",
    "ppm",
    "jxl_container",
    "jxl_headers",
    "jxl_decode",
    "image",
    "roundtrip_png",
    "roundtrip_qoi",
    "roundtrip_bmp",
    "roundtrip_ppm",
    "roundtrip_jxl",
];

/// Generated cases per target in the default campaign, tuned so the whole
/// test takes one to three minutes.
fn default_cases(target: &str) -> u64 {
    match target {
        "png" | "image" | "jxl_decode" => 40_000,
        "jxl_headers" | "jxl_container" | "bmp" | "qoi" | "ppm" => 80_000,
        "roundtrip_jxl" => 8_000,
        _ => 20_000,
    }
}

/// Inputs that once trapped, hung or asked for unbounded scratch, replayed
/// on every run (they also live in the seed corpora). Each entry names the
/// target and the file under fuzz/corpus.
const REGRESSIONS: &[(&str, &str)] = &[
    // FrameInfo.milliseconds() overflowed u32 for long JPEG XL frames.
    (
        "jxl_headers",
        "image_jxl_headers/regress-frame-milliseconds.jxl",
    ),
    (
        "jxl_decode",
        "image_jxl_decode/regress-frame-milliseconds.jxl",
    ),
    // A modular channel one column wider than the weighted predictor rows
    // passed the width check (it counted the rows' padding).
    ("jxl_decode", "image_jxl_decode/regress-wp-row-padding.jxl"),
    // Chromaticities near 2^32 overflowed the primaries match (PNG cHRM).
    ("png", "image_png/regress-chrm-overflow.png"),
    ("image", "image_image/regress-chrm-overflow.png"),
    // The BMP encoder wrote zero-width files its decoder rejects.
    ("roundtrip_bmp", "image_roundtrip_bmp/regress-zero-width"),
];

/// A scratch request is suspicious when it exceeds this many bytes plus
/// SCRATCH_PER_PIXEL per image pixel (the codecs size scratch from the
/// header; growth beyond the image size points at a field that is not
/// validated).
const SCRATCH_BASE: u64 = 32 << 20;
const SCRATCH_PER_PIXEL: u64 = 1024;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name).ok().map(|value| {
        value
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("{name} must be a decimal number"))
    })
}

fn selected_targets() -> Vec<&'static str> {
    match std::env::var("IMAGE_FUZZ_TARGET") {
        Ok(list) if !list.trim().is_empty() => list
            .split(',')
            .map(|name| {
                let name = name.trim();
                *TARGETS
                    .iter()
                    .find(|target| **target == name)
                    .unwrap_or_else(|| {
                        panic!("unknown IMAGE_FUZZ_TARGET {name}; known: {TARGETS:?}")
                    })
            })
            .collect(),
        _ => TARGETS.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Building and running the harness

fn harness() -> &'static Path {
    static HARNESS: OnceLock<PathBuf> = OnceLock::new();
    HARNESS.get_or_init(|| {
        let opt = env_number("IMAGE_FUZZ_OPT").unwrap_or(1);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("image-fuzz");
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join(format!("harness-O{opt}"));
        let status = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args([
                "build",
                "fuzz/image/harness.dodo",
                "-O",
                &opt.to_string(),
                "-o",
            ])
            .arg(&output)
            .current_dir(root())
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "building fuzz/image/harness.dodo failed:\n{}\n{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        output
    })
}

fn timeout() -> Duration {
    Duration::from_secs(env_number("IMAGE_FUZZ_TIMEOUT").unwrap_or(30))
}

/// One input with a description of where it came from.
struct Case {
    data: Vec<u8>,
    origin: String,
}

#[derive(Debug)]
enum Kind {
    Trap(String),
    Hang,
    Scratch { bytes: u64, width: u64, height: u64 },
}

struct Failure {
    target: String,
    origin: String,
    kind: Kind,
    file: PathBuf,
}

#[derive(Default)]
struct Stats {
    cases: usize,
    largest_scratch: u64,
    largest_scratch_size: (u64, u64),
    skipped: u64,
    slowest: Duration,
    elapsed: Duration,
}

fn failure_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("image-fuzz-failures");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save_failure(target: &str, seed: u64, index: usize, case: &Case, kind: Kind) -> Failure {
    let file = failure_dir().join(format!("{target}-seed{seed}-case{index}.bin"));
    std::fs::write(&file, &case.data).unwrap();
    Failure {
        target: target.to_string(),
        origin: format!("seed {seed}, case {index}: {}", case.origin),
        kind,
        file,
    }
}

fn scratch_allowed(bytes: u64, width: u64, height: u64) -> bool {
    bytes
        <= SCRATCH_BASE
            .saturating_add(SCRATCH_PER_PIXEL.saturating_mul(width.saturating_mul(height)))
}

enum Event {
    Line(String, Instant),
    Closed,
}

/// Run `cases` through `target` in as few processes as possible: a trap or
/// hang ends a process, the failing case is recorded and the rest of the
/// batch continues in a new process.
fn run_cases(
    target: &str,
    seed: u64,
    offset: usize,
    cases: &[Case],
    stats: &mut Stats,
) -> Vec<Failure> {
    let mut failures = Vec::new();
    let batch_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("image-fuzz-batches");
    std::fs::create_dir_all(&batch_dir).unwrap();
    static BATCHES: AtomicUsize = AtomicUsize::new(0);
    let batch_number = BATCHES.fetch_add(1, Ordering::Relaxed);
    let batch_path = batch_dir.join(format!(
        "{target}-{seed}-{}-{batch_number}.bin",
        std::process::id()
    ));
    let limit = timeout();
    let started = Instant::now();
    let mut first = 0usize;
    while first < cases.len() {
        let mut batch = Vec::new();
        let mut last = first;
        for case in &cases[first..] {
            if last > first && batch.len() + case.data.len() + 4 > BATCH_BYTES {
                break;
            }
            batch.extend_from_slice(&(case.data.len() as u32).to_le_bytes());
            batch.extend_from_slice(&case.data);
            last += 1;
        }
        std::fs::write(&batch_path, &batch).unwrap();
        let mut child = Command::new(harness())
            .arg(target)
            .arg("--batch")
            .arg(&batch_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if sender.send(Event::Line(line, Instant::now())).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = sender.send(Event::Closed);
        });
        let mut current: Option<usize> = None;
        let mut current_start = Instant::now();
        let mut done = false;
        let mut hung = false;
        loop {
            match receiver.recv_timeout(limit) {
                Ok(Event::Line(line, at)) => {
                    if let Some(index) = line.strip_prefix('@') {
                        if let Some(previous) = current {
                            let _ = previous;
                            stats.slowest = stats.slowest.max(at - current_start);
                        }
                        current = Some(first + index.parse::<usize>().unwrap());
                        current_start = at;
                    } else if let Some(rest) = line.strip_prefix("scratch ") {
                        let fields: Vec<u64> =
                            rest.split(' ').map(|v| v.parse().unwrap()).collect();
                        let index = first + fields[0] as usize;
                        let (bytes, width, height) = (fields[1], fields[2], fields[3]);
                        stats.skipped += fields[4];
                        if bytes > stats.largest_scratch {
                            stats.largest_scratch = bytes;
                            stats.largest_scratch_size = (width, height);
                        }
                        // JPEG XL frames may be larger than the image, and
                        // scratch follows the frames: only the cap applies.
                        let frame_sized = fields.get(5) == Some(&1);
                        if !target.starts_with("roundtrip")
                            && !frame_sized
                            && !scratch_allowed(bytes, width, height)
                        {
                            failures.push(save_failure(
                                target,
                                seed,
                                offset + index,
                                &cases[index],
                                Kind::Scratch {
                                    bytes,
                                    width,
                                    height,
                                },
                            ));
                        }
                    } else if line.starts_with("done ") {
                        done = true;
                    }
                }
                Ok(Event::Closed) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    hung = true;
                    let _ = child.kill();
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let status = child.wait().unwrap();
        let message = errors.join().unwrap();
        if let Some(index) = current {
            stats.slowest = stats.slowest.max(current_start.elapsed().min(limit));
            let _ = index;
        }
        assert!(
            hung || status.code() != Some(2),
            "image harness rejected its batch file {}:\n{message}",
            batch_path.display()
        );
        if done && status.success() && !hung {
            first = last;
            continue;
        }
        let index = current.unwrap_or(first);
        let kind = if hung {
            Kind::Hang
        } else {
            let mut text = message.trim().to_string();
            if text.is_empty() {
                text = format!("{status}");
            }
            Kind::Trap(text)
        };
        failures.push(save_failure(
            target,
            seed,
            offset + index,
            &cases[index],
            kind,
        ));
        first = index + 1;
    }
    let _ = std::fs::remove_file(&batch_path);
    stats.cases += cases.len();
    stats.elapsed += started.elapsed();
    failures
}

/// Failures grouped by target and cause (trap message, hang, scratch), with
/// up to three replayable examples each.
fn report(failures: &[Failure]) -> String {
    let mut groups: BTreeMap<(String, String), Vec<&Failure>> = BTreeMap::new();
    for failure in failures {
        let cause = match &failure.kind {
            Kind::Trap(message) => format!("trap: {}", message.lines().next().unwrap_or("")),
            Kind::Hang => format!("no progress within {}s", timeout().as_secs()),
            Kind::Scratch { .. } => "scratch_size not justified by the image size".to_string(),
        };
        groups
            .entry((failure.target.clone(), cause))
            .or_default()
            .push(failure);
    }
    let mut text = String::new();
    for ((target, cause), members) in &groups {
        text.push_str(&format!(
            "\n[{target}] {cause} ({} inputs)\n",
            members.len()
        ));
        for failure in members.iter().take(3) {
            let detail = match &failure.kind {
                Kind::Scratch {
                    bytes,
                    width,
                    height,
                } => format!(" ({bytes} bytes for {width}x{height})"),
                _ => String::new(),
            };
            text.push_str(&format!(
                "  {}{detail}\n    replay: build/image-harness {target} {}\n",
                failure.origin,
                failure.file.display()
            ));
        }
    }
    text.push_str(
        "\n(build the harness with: dodo build fuzz/image/harness.dodo -o build/image-harness)\n",
    );
    text
}

// ---------------------------------------------------------------------------
// Random numbers and byte helpers

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

fn crc32(data: &[u8]) -> u32 {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (n, entry) in table.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xedb8_8320
                } else {
                    c >> 1
                };
            }
            *entry = c;
        }
        table
    });
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc = table[((crc ^ byte as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A zlib stream of stored deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = data.chunks(65535).peekable();
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Chunks of a PNG: (start, data length) for every complete chunk header.
fn png_chunks(data: &[u8]) -> Vec<(usize, usize)> {
    let mut chunks = Vec::new();
    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return chunks;
    }
    let mut offset = 8;
    while offset + 8 <= data.len() {
        let length = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        chunks.push((offset, length));
        match offset.checked_add(12 + length) {
            Some(next) if next <= data.len() => offset = next,
            _ => break,
        }
    }
    chunks
}

/// Recompute the CRC of every complete chunk.
fn png_fix_crcs(data: &mut [u8]) {
    for (start, length) in png_chunks(data) {
        let end = start + 8 + length;
        if end + 4 <= data.len() {
            let crc = crc32(&data[start + 4..end]);
            data[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        }
    }
}

/// Top-level ISOBMFF boxes: (start, header size, total size).
fn jxl_boxes(data: &[u8]) -> Vec<(usize, usize, usize)> {
    let mut boxes = Vec::new();
    if data.len() < 12 || data[..4] != [0, 0, 0, 12] || &data[4..8] != b"JXL " {
        return boxes;
    }
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let mut size = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as u64;
        let mut header = 8;
        if size == 1 && offset + 16 <= data.len() {
            size = u64::from_be_bytes(data[offset + 8..offset + 16].try_into().unwrap());
            header = 16;
        } else if size == 0 {
            size = (data.len() - offset) as u64;
        }
        if size < header as u64 {
            break;
        }
        boxes.push((offset, header, size as usize));
        match offset.checked_add(size as usize) {
            Some(next) if next <= data.len() => offset = next,
            _ => break,
        }
    }
    boxes
}

/// Offsets where a structure starts or ends: chunk and box boundaries, the
/// header ends of the simple formats, and line ends of PPM headers.
fn boundaries(data: &[u8]) -> Vec<usize> {
    let mut points = Vec::new();
    for (start, length) in png_chunks(data) {
        points.extend([
            start,
            start + 4,
            start + 8,
            start + 8 + length,
            start + 12 + length,
        ]);
    }
    for (start, header, size) in jxl_boxes(data) {
        points.extend([start, start + header, start + size]);
    }
    if data.starts_with(b"BM") {
        points.extend([2, 14, 18, 26, 30, 34, 54, 66, 70, 108, 122, 138]);
        if data.len() >= 18 {
            points.push(14 + u32::from_le_bytes(data[14..18].try_into().unwrap()) as usize);
        }
        if data.len() >= 14 {
            points.push(u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize);
        }
    }
    if data.starts_with(b"qoif") {
        points.extend([4, 8, 12, 13, 14]);
        if data.len() >= 8 {
            points.push(data.len() - 8);
        }
    }
    if data.first() == Some(&b'P') {
        for (i, &byte) in data.iter().enumerate().take(512) {
            if byte == b'\n' || byte == b' ' {
                points.push(i + 1);
            }
        }
    }
    if data.starts_with(&[0xff, 0x0a]) {
        points.extend([2, 3, 4, 5, 6, 8, 12, 16, 24, 32, 48, 64]);
    }
    points.retain(|&p| p > 0 && p < data.len());
    points.sort_unstable();
    points.dedup();
    points
}

// ---------------------------------------------------------------------------
// Mutations

const INTERESTING8: &[u8] = &[
    0, 1, 2, 7, 8, 15, 16, 31, 32, 63, 64, 0x7f, 0x80, 0x81, 0xfe, 0xff,
];
const INTERESTING32: &[u32] = &[
    0,
    1,
    2,
    3,
    4,
    8,
    12,
    16,
    255,
    256,
    4095,
    4096,
    65535,
    65536,
    0x7fff,
    0x8000,
    0x00ff_ffff,
    0x0100_0000,
    0x7fff_ffff,
    0x8000_0000,
    0xffff_fffe,
    0xffff_ffff,
];

fn put32(data: &mut [u8], at: usize, value: u32, big: bool) {
    let bytes = if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    for (i, byte) in bytes.iter().enumerate() {
        if at + i < data.len() {
            data[at + i] = *byte;
        }
    }
}

fn mutate_once(rng: &mut Rng, data: &mut Vec<u8>, pool: &[Case], log: &mut String) {
    if data.is_empty() {
        data.extend({
            let n = 1 + rng.below(16);
            rng.bytes(n)
        });
        log.push_str(" random");
        return;
    }
    let len = data.len();
    // Bias positions toward the headers, where most of the parsing is.
    let position = |rng: &mut Rng| {
        if rng.chance(2) {
            rng.below(len.min(128))
        } else {
            rng.below(len)
        }
    };
    match rng.below(12) {
        0 | 1 => {
            let flips = 1 + rng.below(8);
            for _ in 0..flips {
                let at = position(rng);
                data[at] ^= 1 << rng.below(8);
            }
            log.push_str(&format!(" flip{flips}"));
        }
        2 | 3 => {
            let at = position(rng);
            data[at] = INTERESTING8[rng.below(INTERESTING8.len())];
            log.push_str(&format!(" byte@{at}"));
        }
        4 => {
            let at = position(rng);
            let mut value = INTERESTING32[rng.below(INTERESTING32.len())];
            if rng.chance(3) {
                value = (len - at.min(len)) as u32 + rng.below(3) as u32 - 1;
            }
            let big = rng.chance(2);
            if rng.chance(2) {
                put32(data, at, value, big);
                log.push_str(&format!(" u32@{at}={value:#x}"));
            } else {
                let half = value as u16;
                let bytes = if big {
                    half.to_be_bytes()
                } else {
                    half.to_le_bytes()
                };
                data[at] = bytes[0];
                if at + 1 < len {
                    data[at + 1] = bytes[1];
                }
                log.push_str(&format!(" u16@{at}={half:#x}"));
            }
        }
        5 => {
            // Truncate at a structure boundary or anywhere.
            let points = boundaries(data);
            let at = if !points.is_empty() && rng.chance(2) {
                points[rng.below(points.len())]
            } else {
                rng.below(len)
            };
            data.truncate(at);
            log.push_str(&format!(" cut@{at}"));
        }
        6 => {
            // Corrupt a chunk or box length field.
            let chunks = png_chunks(data);
            let boxes = jxl_boxes(data);
            let value = if rng.chance(2) {
                INTERESTING32[rng.below(INTERESTING32.len())]
            } else {
                (len as u32)
                    .wrapping_add(rng.below(64) as u32)
                    .wrapping_sub(32)
            };
            if !chunks.is_empty() {
                let (start, length) = chunks[rng.below(chunks.len())];
                let value = if rng.chance(2) {
                    (length as u32)
                        .wrapping_add(rng.below(9) as u32)
                        .wrapping_sub(4)
                } else {
                    value
                };
                put32(data, start, value, true);
                log.push_str(&format!(" chunklen@{start}={value:#x}"));
            } else if !boxes.is_empty() {
                let (start, _, size) = boxes[rng.below(boxes.len())];
                let value = if rng.chance(2) {
                    (size as u32)
                        .wrapping_add(rng.below(9) as u32)
                        .wrapping_sub(4)
                } else {
                    value
                };
                put32(data, start, value, true);
                log.push_str(&format!(" boxlen@{start}={value:#x}"));
            } else {
                let at = position(rng);
                put32(data, at, value, rng.chance(2));
                log.push_str(&format!(" len@{at}={value:#x}"));
            }
        }
        7 => {
            // Damage a chunk's payload; the CRC is fixed below.
            let chunks = png_chunks(data);
            if let Some(&(start, length)) = chunks.get(rng.below(chunks.len().max(1)))
                && length > 0
                && start + 8 + length <= len
            {
                let at = start + 8 + rng.below(length.min(64));
                data[at] = if rng.chance(2) {
                    INTERESTING8[rng.below(INTERESTING8.len())]
                } else {
                    data[at] ^ (1 << rng.below(8))
                };
                log.push_str(&format!(" chunkdata@{at}"));
                return;
            }
            let at = position(rng);
            data[at] = rng.next() as u8;
            log.push_str(&format!(" set@{at}"));
        }
        8 => {
            // Duplicate or remove a chunk or box.
            let mut spans: Vec<(usize, usize)> = png_chunks(data)
                .iter()
                .filter(|(start, length)| start + 12 + length <= len)
                .map(|&(start, length)| (start, 12 + length))
                .collect();
            spans.extend(
                jxl_boxes(data)
                    .iter()
                    .filter(|b| b.0 + b.2 <= len)
                    .map(|&(start, _, size)| (start, size)),
            );
            if spans.is_empty() {
                let at = rng.below(len);
                let count = 1 + rng.below(16.min(len - at));
                let copy = data[at..at + count].to_vec();
                let to = rng.below(len);
                data.splice(to..to, copy);
                log.push_str(&format!(" dup{count}@{at}"));
                return;
            }
            let (start, size) = spans[rng.below(spans.len())];
            if rng.chance(2) {
                data.drain(start..start + size);
                log.push_str(&format!(" drop@{start}"));
            } else {
                let copy = data[start..start + size].to_vec();
                let (to, _) = spans[rng.below(spans.len())];
                data.splice(to..to, copy);
                log.push_str(&format!(" dupchunk@{start}->{to}"));
            }
        }
        9 => {
            // Splice: this input's head with another's tail, at boundaries
            // when there are some.
            if pool.is_empty() {
                return;
            }
            let other = &pool[rng.below(pool.len())];
            let mine = boundaries(data);
            let theirs = boundaries(&other.data);
            let cut = if mine.is_empty() {
                rng.below(len)
            } else {
                mine[rng.below(mine.len())]
            };
            let from = if theirs.is_empty() {
                rng.below(other.data.len().max(1))
            } else {
                theirs[rng.below(theirs.len())]
            };
            data.truncate(cut);
            data.extend_from_slice(&other.data[from.min(other.data.len())..]);
            log.push_str(&format!(" splice@{cut}+{}@{from}", other.origin));
        }
        10 => {
            let at = rng.below(len);
            let count = 1 + rng.below(32.min(len - at));
            if rng.chance(2) {
                data.drain(at..at + count);
                log.push_str(&format!(" del{count}@{at}"));
            } else {
                let bytes = rng.bytes(count);
                data.splice(at..at, bytes);
                log.push_str(&format!(" ins{count}@{at}"));
            }
        }
        _ => {
            // Overwrite a run with one value (zero-filled or 0xff areas).
            let at = position(rng);
            let count = 1 + rng.below(64.min(len - at));
            let value = INTERESTING8[rng.below(INTERESTING8.len())];
            for byte in &mut data[at..at + count] {
                *byte = value;
            }
            log.push_str(&format!(" fill{count}@{at}"));
        }
    }
}

fn mutate(rng: &mut Rng, base: &Case, pool: &[Case]) -> Case {
    let mut data = base.data.clone();
    let mut log = base.origin.clone();
    let rounds = 1 + rng.below(3);
    for _ in 0..rounds {
        mutate_once(rng, &mut data, pool, &mut log);
    }
    if data.starts_with(&PNG_SIGNATURE) && !rng.chance(4) {
        png_fix_crcs(&mut data);
        log.push_str(" crcfix");
    }
    data.truncate(MAX_INPUT);
    Case { data, origin: log }
}

// ---------------------------------------------------------------------------
// Structure-aware generators

/// A PNG (sometimes APNG) with valid CRCs and a stored zlib stream around
/// random filtered rows, so damage reaches unfiltering, deinterlacing,
/// palette expansion and frame composition.
fn generate_png(rng: &mut Rng) -> Case {
    let types: [(u8, &[u8]); 5] = [
        (0, &[1, 2, 4, 8, 16]),
        (2, &[8, 16]),
        (3, &[1, 2, 4, 8]),
        (4, &[8, 16]),
        (6, &[8, 16]),
    ];
    let (color, depths) = types[rng.below(types.len())];
    let mut depth = depths[rng.below(depths.len())];
    if rng.chance(40) {
        depth = rng.next() as u8;
    }
    let width = 1 + {
        let n = if rng.chance(8) { 300 } else { 24 };
        rng.below(n)
    } as u32;
    let height = 1 + {
        let n = if rng.chance(8) { 300 } else { 24 };
        rng.below(n)
    } as u32;
    let interlace = rng.chance(3) as u8;
    let channels = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let bits = channels * depth.max(1) as usize;
    let mut out = PNG_SIGNATURE.to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[depth, color, 0, 0, interlace]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    if color == 3 || (rng.chance(6) && color != 0 && color != 4) {
        let entries = 1 + {
            let n = if rng.chance(4) {
                300
            } else {
                1 << depth.min(8)
            };
            rng.below(n)
        };
        png_chunk(&mut out, b"PLTE", &rng.bytes(entries * 3));
    }
    if rng.chance(3) {
        let length = match color {
            0 => 2,
            2 => 6,
            3 => 1 + rng.below(260),
            _ => rng.below(8),
        };
        png_chunk(&mut out, b"tRNS", &rng.bytes(length));
    }
    for _ in 0..rng.below(3) {
        let (kind, data): (&[u8; 4], Vec<u8>) = match rng.below(8) {
            0 => (b"gAMA", rng.bytes(4)),
            1 => (b"cHRM", rng.bytes(32)),
            2 => (b"sRGB", vec![rng.below(5) as u8]),
            3 => (b"iCCP", {
                let mut v = b"icc\0\0".to_vec();
                v.extend(zlib_stored(&{
                    let n = rng.below(200);
                    rng.bytes(n)
                }));
                v
            }),
            4 => (b"eXIf", {
                let mut v = b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
                v.extend({
                    let n = rng.below(12);
                    rng.bytes(n)
                });
                v
            }),
            5 => (b"cICP", rng.bytes(4)),
            6 => (b"iTXt", {
                let mut v = b"XML:com.adobe.xmp\0".to_vec();
                v.push(rng.below(2) as u8);
                v.push(0);
                v.extend_from_slice(b"\0\0");
                v.extend({
                    let n = rng.below(40);
                    rng.bytes(n)
                });
                v
            }),
            _ => (b"sBIT", {
                let n = 1 + rng.below(4);
                rng.bytes(n)
            }),
        };
        png_chunk(&mut out, kind, &data);
    }
    let raw_image = |rng: &mut Rng, w: u32, h: u32| -> Vec<u8> {
        let row = (w as usize * bits).div_ceil(8);
        let mut raw = Vec::new();
        let rows = if interlace == 1 {
            h as usize * 2 + 14
        } else {
            h as usize
        };
        for _ in 0..rows {
            raw.push(if rng.chance(10) {
                rng.next() as u8
            } else {
                rng.below(5) as u8
            });
            if rng.chance(3) {
                raw.extend(std::iter::repeat_n(rng.next() as u8, row));
            } else {
                raw.extend(rng.bytes(row));
            }
        }
        if rng.chance(4) {
            let keep = rng.below(raw.len() + 1);
            raw.truncate(keep);
        }
        raw
    };
    let animated = rng.chance(3);
    let frames = if animated { 1 + rng.below(4) } else { 0 };
    let mut sequence = 0u32;
    let default_is_frame = animated && rng.chance(2);
    if animated {
        let mut actl = (frames as u32).to_be_bytes().to_vec();
        if rng.chance(10) {
            actl = (rng.next() as u32).to_be_bytes().to_vec();
        }
        actl.extend_from_slice(&(rng.below(3) as u32).to_be_bytes());
        png_chunk(&mut out, b"acTL", &actl);
    }
    let fctl = |rng: &mut Rng, sequence: &mut u32, full: bool| -> (Vec<u8>, u32, u32) {
        let (fw, fh, x, y) = if full || rng.chance(3) {
            (width, height, 0, 0)
        } else {
            let fw = 1 + rng.below(width as usize) as u32;
            let fh = 1 + rng.below(height as usize) as u32;
            let x = rng.below((width - fw + 1) as usize) as u32;
            let y = rng.below((height - fh + 1) as usize) as u32;
            if rng.chance(12) {
                (fw + 1, fh, x + rng.below(3) as u32, y)
            } else {
                (fw, fh, x, y)
            }
        };
        let mut v = sequence.to_be_bytes().to_vec();
        *sequence += 1;
        for value in [fw, fh, x, y] {
            v.extend_from_slice(&value.to_be_bytes());
        }
        v.extend_from_slice(&(rng.below(1000) as u16).to_be_bytes());
        v.extend_from_slice(&(rng.below(200) as u16).to_be_bytes());
        v.push({
            let n = if rng.chance(10) { 256 } else { 3 };
            rng.below(n)
        } as u8);
        v.push({
            let n = if rng.chance(10) { 256 } else { 2 };
            rng.below(n)
        } as u8);
        (v, fw, fh)
    };
    if default_is_frame {
        let (v, _, _) = fctl(rng, &mut sequence, true);
        png_chunk(&mut out, b"fcTL", &v);
    }
    let raw = raw_image(rng, width, height);
    let stream = zlib_stored(&raw);
    // Split IDAT sometimes.
    if stream.len() > 4 && rng.chance(3) {
        let cut = rng.below(stream.len());
        png_chunk(&mut out, b"IDAT", &stream[..cut]);
        png_chunk(&mut out, b"IDAT", &stream[cut..]);
    } else {
        png_chunk(&mut out, b"IDAT", &stream);
    }
    let rest = if default_is_frame {
        frames.saturating_sub(1)
    } else {
        frames
    };
    for _ in 0..rest {
        let (v, fw, fh) = fctl(rng, &mut sequence, false);
        png_chunk(&mut out, b"fcTL", &v);
        let raw = raw_image(rng, fw, fh);
        let mut data = sequence.to_be_bytes().to_vec();
        sequence += 1;
        data.extend(zlib_stored(&raw));
        png_chunk(&mut out, b"fdAT", &data);
    }
    png_chunk(&mut out, b"IEND", &[]);
    Case {
        data: out,
        origin: format!(
            "generated png type {color} depth {depth} {width}x{height} interlace {interlace} frames {frames}"
        ),
    }
}

/// A BMP with a random header variant (core/info/V4/V5, bit depth,
/// compression, masks, palette) and random pixel or RLE data.
fn generate_bmp(rng: &mut Rng) -> Case {
    let header_sizes = [12u32, 40, 52, 56, 108, 124, 64];
    let header_size = header_sizes[rng.below(header_sizes.len())];
    let bits_choices = [1u16, 4, 8, 16, 24, 32, 2, 0];
    let bits = bits_choices[rng.below(bits_choices.len())];
    let compression = match rng.below(6) {
        0 | 1 => 0u32,
        2 => 1,
        3 => 2,
        4 => 3,
        _ => [6u32, 4, 5, 7][rng.below(4)],
    };
    let width = 1 + {
        let n = if rng.chance(8) { 200 } else { 20 };
        rng.below(n)
    } as i32;
    let mut height = 1 + {
        let n = if rng.chance(8) { 200 } else { 20 };
        rng.below(n)
    } as i32;
    if rng.chance(4) {
        height = -height;
    }
    let palette = if bits <= 8 {
        rng.below(1 + (1usize << bits.min(8)))
    } else {
        rng.below(3)
    };
    let entry = if header_size == 12 { 3 } else { 4 };
    let mut header = Vec::new();
    header.extend_from_slice(&header_size.to_le_bytes());
    if header_size == 12 {
        header.extend_from_slice(&(width as u16).to_le_bytes());
        header.extend_from_slice(&(height as u16).to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&bits.to_le_bytes());
    } else {
        header.extend_from_slice(&width.to_le_bytes());
        header.extend_from_slice(&height.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&bits.to_le_bytes());
        header.extend_from_slice(&compression.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&2835u32.to_le_bytes());
        header.extend_from_slice(&2835u32.to_le_bytes());
        header.extend_from_slice(&(palette as u32).to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        let masks: [u32; 4] = match rng.below(4) {
            0 => [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000],
            1 => [0x7c00, 0x03e0, 0x001f, 0x8000],
            2 => [0xf800, 0x07e0, 0x001f, 0],
            _ => [
                rng.next() as u32,
                rng.next() as u32,
                rng.next() as u32,
                rng.next() as u32,
            ],
        };
        for mask in masks {
            if header.len() < header_size as usize {
                header.extend_from_slice(&mask.to_le_bytes());
            }
        }
        if header_size >= 108 && header.len() < header_size as usize {
            let space: u32 = if rng.chance(2) {
                0x7352_4742
            } else {
                0x4d42_4544
            };
            header.extend_from_slice(&space.to_le_bytes());
        }
        while header.len() < header_size as usize {
            header.push(if rng.chance(4) { rng.next() as u8 } else { 0 });
        }
        if header_size >= 124 {
            // Profile offset and size, relative to the info header.
            let profile_offset = header_size + rng.below(64) as u32;
            header[112..116].copy_from_slice(&profile_offset.to_le_bytes());
            header[116..120].copy_from_slice(&(rng.below(128) as u32).to_le_bytes());
        }
    }
    let mut body = rng.bytes(palette * entry);
    let row = ((width as usize * bits.max(1) as usize).div_ceil(32)) * 4;
    let rows = height.unsigned_abs() as usize;
    if compression == 1 || compression == 2 {
        // RLE: runs, literal runs and escapes.
        for _ in 0..rng.below(rows * 4 + 8) {
            match rng.below(6) {
                0 => body.extend_from_slice(&[0, 0]),
                1 => body.extend_from_slice(&[0, 2, rng.below(8) as u8, rng.below(8) as u8]),
                2 => {
                    let count = 3 + rng.below(20);
                    body.extend_from_slice(&[0, count as u8]);
                    let bytes = if compression == 1 {
                        count
                    } else {
                        count.div_ceil(2)
                    };
                    body.extend(rng.bytes(bytes + (bytes & 1)));
                }
                _ => body.extend_from_slice(&[1 + rng.below(30) as u8, rng.next() as u8]),
            }
        }
        if rng.chance(2) {
            body.extend_from_slice(&[0, 1]);
        }
    } else {
        let mut size = row * rows;
        if rng.chance(4) {
            size = rng.below(size + 1);
        }
        body.extend(rng.bytes(size));
    }
    let offset = 14 + header.len() + palette * entry;
    let mut out = b"BM".to_vec();
    out.extend_from_slice(&((14 + header.len() + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    let stored_offset = if rng.chance(10) {
        rng.next() as u32 % 512
    } else {
        offset as u32
    };
    out.extend_from_slice(&stored_offset.to_le_bytes());
    out.extend(header);
    out.extend(body);
    Case {
        data: out,
        origin: format!(
            "generated bmp header {header_size} bits {bits} compression {compression} {width}x{height}"
        ),
    }
}

/// A QOI header and a random op stream.
fn generate_qoi(rng: &mut Rng) -> Case {
    let width = 1 + {
        let n = if rng.chance(8) { 400 } else { 32 };
        rng.below(n)
    } as u32;
    let height = 1 + {
        let n = if rng.chance(8) { 400 } else { 32 };
        rng.below(n)
    } as u32;
    let mut out = b"qoif".to_vec();
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.push(if rng.chance(8) {
        rng.next() as u8
    } else {
        3 + rng.below(2) as u8
    });
    out.push(if rng.chance(8) {
        rng.next() as u8
    } else {
        rng.below(2) as u8
    });
    let pixels = (width * height) as usize;
    let mut produced = 0;
    while produced < pixels && out.len() < 1 << 20 {
        match rng.below(8) {
            0 => {
                out.push(0xfe);
                out.extend(rng.bytes(3));
                produced += 1;
            }
            1 => {
                out.push(0xff);
                out.extend(rng.bytes(4));
                produced += 1;
            }
            2 => {
                let run = rng.below(62);
                out.push(0xc0 | run as u8);
                produced += run + 1;
            }
            3 => {
                out.push(0x80 | rng.below(64) as u8);
                out.push(rng.next() as u8);
                produced += 1;
            }
            _ => {
                out.push(rng.next() as u8 & 0x7f);
                produced += 1;
            }
        }
        if rng.chance(500) {
            break;
        }
    }
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
    Case {
        data: out,
        origin: format!("generated qoi {width}x{height}"),
    }
}

/// A PNM/PAM/PFM header with odd but legal spacing, comments and values.
fn generate_ppm(rng: &mut Rng) -> Case {
    let magic = [
        b"P1", b"P2", b"P3", b"P4", b"P5", b"P6", b"P7", b"PF", b"Pf", b"PH",
    ][rng.below(10)];
    let space = |rng: &mut Rng| -> Vec<u8> {
        let mut v = Vec::new();
        for _ in 0..1 + rng.below(2) {
            match rng.below(5) {
                0 => v.extend_from_slice(b"\n# comment\n"),
                1 => v.push(b'\t'),
                2 => v.push(b'\r'),
                _ => v.push(b' '),
            }
        }
        v
    };
    let number = |rng: &mut Rng| -> Vec<u8> {
        let value: u64 = match rng.below(6) {
            0 => rng.next() % 5,
            1 => [255, 256, 65535, 65536, 1, 0][rng.below(6)],
            2 => rng.next() % 100_000_000_000,
            _ => 1 + rng.next() % 40,
        };
        value.to_string().into_bytes()
    };
    let mut out = magic.to_vec();
    if magic == b"P7" {
        out.push(b'\n');
        for key in [&b"WIDTH"[..], b"HEIGHT", b"DEPTH", b"MAXVAL"] {
            if !rng.chance(12) {
                out.extend_from_slice(key);
                out.push(b' ');
                out.extend(number(rng));
                out.push(b'\n');
            }
        }
        if rng.chance(2) {
            let types = [
                &b"GRAYSCALE"[..],
                b"RGB",
                b"RGB_ALPHA",
                b"GRAYSCALE_ALPHA",
                b"BLACKANDWHITE",
                b"BLACKANDWHITE_ALPHA",
                b"X",
            ];
            out.extend_from_slice(b"TUPLTYPE ");
            out.extend_from_slice(types[rng.below(types.len())]);
            out.push(b'\n');
        }
        out.extend_from_slice(b"ENDHDR\n");
    } else {
        out.extend(space(rng));
        out.extend(number(rng));
        out.extend(space(rng));
        out.extend(number(rng));
        if magic != b"P1" && magic != b"P4" {
            out.extend(space(rng));
            if magic == b"PF" || magic == b"Pf" {
                out.extend_from_slice(
                    [&b"-1.0"[..], b"1.0", b"0", b"-0.5", b"nan", b"1e40"][rng.below(6)],
                );
            } else {
                out.extend(number(rng));
            }
        }
        out.push(b'\n');
    }
    let body = {
        let n = if rng.chance(4) { 20000 } else { 600 };
        rng.below(n)
    };
    if magic == b"P1" || magic == b"P2" || magic == b"P3" {
        for _ in 0..body / 3 {
            out.extend(number(rng));
            out.push(b' ');
        }
    } else {
        out.extend(rng.bytes(body));
    }
    Case {
        data: out,
        origin: format!("generated {}", String::from_utf8_lossy(magic)),
    }
}

/// Round-trip parameters and samples (see fuzz/image/targets/roundtrip.dodo).
fn generate_roundtrip(rng: &mut Rng) -> Case {
    let mut head = rng.bytes(7);
    // Mostly small images; sometimes empty or larger ones.
    if !rng.chance(8) {
        head[1] = rng.below(24) as u8;
        head[2] = rng.below(24) as u8;
    }
    if rng.chance(2) {
        head[5] = 32 + rng.below(200) as u8;
    }
    let samples = match rng.below(4) {
        0 => {
            let n = rng.below(16);
            rng.bytes(n)
        }
        1 => {
            // Few distinct values (palette, runs, QOI index hits).
            let values = {
                let n = 1 + rng.below(6);
                rng.bytes(n)
            };
            (0..rng.below(4096))
                .map(|_| values[rng.below(values.len())])
                .collect()
        }
        2 => {
            // Float specials: NaN, infinities, negative, huge, denormals.
            let specials: [u32; 8] = [
                0x7fc0_0000,
                0x7f80_0000,
                0xff80_0000,
                0xbf80_0000,
                0x3f80_0000,
                0x7f7f_ffff,
                0x0000_0001,
                0x4f00_0000,
            ];
            (0..rng.below(1024))
                .flat_map(|_| specials[rng.below(8)].to_le_bytes())
                .collect()
        }
        _ => {
            let n = rng.below(8192);
            rng.bytes(n)
        }
    };
    head.extend(samples);
    Case {
        data: head,
        origin: "generated roundtrip".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Corpora

fn files_in(dir: &Path, extensions: &[&str], out: &mut Vec<Case>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    for path in paths {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extensions.is_empty() || extensions.contains(&extension.as_str()) {
            let data = std::fs::read(&path).unwrap();
            let origin = path
                .strip_prefix(root())
                .unwrap_or(&path)
                .display()
                .to_string();
            out.push(Case { data, origin });
        }
    }
}

/// The tracked seeds of a target: fuzz/corpus/image_<target>.
fn seed_corpus(target: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    files_in(
        &root().join("fuzz/corpus").join(format!("image_{target}")),
        &[],
        &mut cases,
    );
    cases
}

/// Seeds plus the test fixtures a target understands.
fn base_corpus(target: &str) -> Vec<Case> {
    let mut cases = seed_corpus(target);
    let root = root();
    let fixtures = root.join("tests/image/codecs/fixtures");
    let png_dirs = |out: &mut Vec<Case>| {
        files_in(&root.join("tests/graphics/png"), &["png"], out);
        files_in(&fixtures, &["png"], out);
    };
    let jxl_dirs = |out: &mut Vec<Case>| {
        files_in(&root.join("tests/image/jxl"), &["jxl"], out);
        files_in(&root.join("tests/image/jxl_encode/golden"), &["jxl"], out);
    };
    match target {
        "png" => png_dirs(&mut cases),
        "bmp" => files_in(&fixtures, &["bmp"], &mut cases),
        "qoi" => files_in(&fixtures, &["qoi"], &mut cases),
        "ppm" => files_in(
            &fixtures,
            &["ppm", "pgm", "pbm", "pnm", "pam", "pfm"],
            &mut cases,
        ),
        "jxl_container" | "jxl_headers" | "jxl_decode" => jxl_dirs(&mut cases),
        "image" => {
            png_dirs(&mut cases);
            jxl_dirs(&mut cases);
            files_in(&fixtures, &["bmp", "qoi", "ppm", "pam", "pfm"], &mut cases);
        }
        _ => {}
    }
    cases
}

fn generate(target: &str, rng: &mut Rng) -> Option<Case> {
    let pick = if target == "image" {
        ["png", "bmp", "qoi", "ppm"][rng.below(4)]
    } else {
        target
    };
    match pick {
        "png" => Some(generate_png(rng)),
        "bmp" => Some(generate_bmp(rng)),
        "qoi" => Some(generate_qoi(rng)),
        "ppm" => Some(generate_ppm(rng)),
        t if t.starts_with("roundtrip") => Some(generate_roundtrip(rng)),
        _ => None,
    }
}

/// Where a target's inputs come from: a fixed list (seeds, replay), or a
/// campaign that yields every base input, truncations of each at every
/// structure boundary, then generated and mutated inputs from its seed. The
/// campaign produces its inputs batch by batch so memory stays small.
struct Source {
    fixed: std::vec::IntoIter<Case>,
    pool: Vec<Case>,
    rng: Rng,
    generated: u64,
    limit: u64,
    // How long the campaign runs, counted from its first input.
    duration: Option<Duration>,
    deadline: Option<Instant>,
    target: &'static str,
}

impl Source {
    fn fixed(target: &'static str, cases: Vec<Case>) -> Source {
        Source {
            fixed: cases.into_iter(),
            pool: Vec::new(),
            rng: Rng::new(0),
            generated: 0,
            limit: 0,
            duration: None,
            deadline: None,
            target,
        }
    }

    fn campaign(target: &'static str, seed: u64, limit: u64, duration: Option<Duration>) -> Source {
        let base = base_corpus(target);
        let mut fixed = Vec::new();
        for case in &base {
            fixed.push(Case {
                data: case.data.clone(),
                origin: case.origin.clone(),
            });
        }
        if seed == 0 {
            for case in &base {
                for point in boundaries(&case.data) {
                    fixed.push(Case {
                        data: case.data[..point].to_vec(),
                        origin: format!("{} cut@{point}", case.origin),
                    });
                }
            }
        }
        let index = TARGETS.iter().position(|t| *t == target).unwrap() as u64;
        let rng = Rng::new(seed.wrapping_mul(0x0100_0000_01b3) ^ (index << 56));
        Source {
            fixed: fixed.into_iter(),
            pool: base,
            rng,
            generated: 0,
            limit,
            duration,
            deadline: None,
            target,
        }
    }

    fn next_case(&mut self) -> Option<Case> {
        if self.deadline.is_none() {
            self.deadline = self.duration.map(|d| Instant::now() + d);
        }
        if let Some(case) = self.fixed.next() {
            return Some(case);
        }
        if self.generated >= self.limit || self.deadline.is_some_and(|d| Instant::now() >= d) {
            return None;
        }
        self.generated += 1;
        let rng = &mut self.rng;
        let generate_new = self.pool.is_empty() || rng.chance(4);
        let mut fresh = None;
        let case = if generate_new {
            match generate(self.target, rng) {
                Some(case) => {
                    let mutated = if rng.chance(2) {
                        Some(mutate(rng, &case, &self.pool))
                    } else {
                        None
                    };
                    fresh = Some(Case {
                        data: case.data.clone(),
                        origin: case.origin.clone(),
                    });
                    mutated.unwrap_or(case)
                }
                None => {
                    let parent = &self.pool[rng.below(self.pool.len())];
                    mutate(rng, parent, &self.pool)
                }
            }
        } else {
            let parent = &self.pool[rng.below(self.pool.len())];
            mutate(rng, parent, &self.pool)
        };
        // Generated inputs (not their mutations, which would compound) also
        // seed later mutations.
        if let Some(entry) = fresh {
            if self.pool.len() < 1024 {
                self.pool.push(entry);
            } else {
                let at = self.rng.below(self.pool.len());
                self.pool[at] = entry;
            }
        }
        Some(case)
    }

    fn next_batch(&mut self, size: usize) -> Vec<Case> {
        let mut batch = Vec::new();
        while batch.len() < size {
            match self.next_case() {
                Some(case) => batch.push(case),
                None => break,
            }
        }
        batch
    }
}

const BATCH: usize = 1000;
/// Mutated inputs are cut to this size; a batch file stays below the
/// harness's 64 MiB input buffer.
const MAX_INPUT: usize = 1 << 20;
const BATCH_BYTES: usize = 32 << 20;

// ---------------------------------------------------------------------------
// Tests

fn run_targets(work: Vec<Source>, seed: u64) -> (Vec<Failure>, BTreeMap<&'static str, Stats>) {
    let jobs = env_number("IMAGE_FUZZ_JOBS")
        .map(|j| j as usize)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .min(4)
        })
        .max(1);
    harness();
    let queue = Arc::new(Mutex::new(work));
    let results = Arc::new(Mutex::new((Vec::new(), BTreeMap::new())));
    let mut workers = Vec::new();
    for _ in 0..jobs {
        let queue = Arc::clone(&queue);
        let results = Arc::clone(&results);
        workers.push(std::thread::spawn(move || {
            loop {
                let Some(mut source) = queue.lock().unwrap().pop() else {
                    return;
                };
                let target = source.target;
                let mut stats = Stats::default();
                let mut failures = Vec::new();
                let mut offset = 0;
                loop {
                    let batch = source.next_batch(BATCH);
                    if batch.is_empty() {
                        break;
                    }
                    failures.extend(run_cases(target, seed, offset, &batch, &mut stats));
                    offset += batch.len();
                }
                let mut guard = results.lock().unwrap();
                guard.0.extend(failures);
                guard.1.insert(target, stats);
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    let mut guard = results.lock().unwrap();
    (std::mem::take(&mut guard.0), std::mem::take(&mut guard.1))
}

fn print_stats(stats: &BTreeMap<&'static str, Stats>) {
    for (target, s) in stats {
        eprintln!(
            "image_fuzz {target:>14}: {:>6} cases in {:>6.1}s, slowest {:>5.2}s, largest scratch {} ({}x{}), {} skipped at caps",
            s.cases,
            s.elapsed.as_secs_f64(),
            s.slowest.as_secs_f64(),
            s.largest_scratch,
            s.largest_scratch_size.0,
            s.largest_scratch_size.1,
            s.skipped
        );
    }
}

/// Seed corpora and the regression inputs of fixed bugs, as they are.
#[test]
fn image_fuzz_seed_corpora_and_regressions() {
    for (target, file) in REGRESSIONS {
        assert!(
            TARGETS.contains(target),
            "regression for unknown target {target}"
        );
        let path = root().join("fuzz/corpus").join(file);
        assert!(
            path.is_file(),
            "regression input {} is missing",
            path.display()
        );
    }
    let mut work = Vec::new();
    for target in TARGETS {
        let cases = seed_corpus(target);
        assert!(!cases.is_empty(), "fuzz/corpus/image_{target} has no seeds");
        work.push(Source::fixed(target, cases));
    }
    for (target, file) in REGRESSIONS {
        let data = std::fs::read(root().join("fuzz/corpus").join(file)).unwrap();
        work.push(Source::fixed(
            target,
            vec![Case {
                data,
                origin: format!("regression {file}"),
            }],
        ));
    }
    let (failures, _) = run_targets(work, 0);
    assert!(
        failures.is_empty(),
        "image fuzz seeds failed:{}",
        report(&failures)
    );
}

/// The seeded campaign; IMAGE_FUZZ_INPUT replays files instead.
#[test]
fn image_fuzz_campaign() {
    if let Some(input) = std::env::var_os("IMAGE_FUZZ_INPUT") {
        let input = PathBuf::from(input);
        let mut cases = Vec::new();
        if input.is_dir() {
            files_in(&input, &[], &mut cases);
        } else {
            cases.push(Case {
                data: std::fs::read(&input).expect("read IMAGE_FUZZ_INPUT"),
                origin: input.display().to_string(),
            });
        }
        let work = selected_targets()
            .into_iter()
            .map(|target| {
                Source::fixed(
                    target,
                    cases
                        .iter()
                        .map(|c| Case {
                            data: c.data.clone(),
                            origin: c.origin.clone(),
                        })
                        .collect(),
                )
            })
            .collect();
        let (failures, stats) = run_targets(work, 0);
        print_stats(&stats);
        assert!(
            failures.is_empty(),
            "image fuzz replay failed:{}",
            report(&failures)
        );
        return;
    }
    let seed = env_number("IMAGE_FUZZ_SEED").unwrap_or(0);
    let override_cases = env_number("IMAGE_FUZZ_CASES");
    // With IMAGE_FUZZ_SECONDS each target runs that long (and at most
    // IMAGE_FUZZ_CASES generated inputs when that is set too).
    let seconds = env_number("IMAGE_FUZZ_SECONDS");
    let mut work = Vec::new();
    for target in selected_targets() {
        let count = match (override_cases, seconds) {
            (Some(count), _) => count,
            (None, Some(_)) => u64::MAX,
            (None, None) => default_cases(target),
        };
        work.push(Source::campaign(
            target,
            seed,
            count,
            seconds.map(Duration::from_secs),
        ));
    }
    if let Some(dir) = std::env::var_os("IMAGE_FUZZ_DUMP") {
        // Write the inputs instead of running them.
        for mut source in work {
            let dir = PathBuf::from(&dir).join(source.target);
            std::fs::create_dir_all(&dir).unwrap();
            let mut index = 0;
            while let Some(case) = source.next_case() {
                std::fs::write(dir.join(format!("{index:07}.bin")), &case.data).unwrap();
                index += 1;
            }
        }
        return;
    }
    let (failures, stats) = run_targets(work, seed);
    print_stats(&stats);
    assert!(
        failures.is_empty(),
        "image fuzz campaign (seed {seed}) failed:{}",
        report(&failures)
    );
}
