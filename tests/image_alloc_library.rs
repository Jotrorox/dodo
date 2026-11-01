//! std/image_alloc: the Dodo tests in tests/image/alloc (owned images in a
//! shared arena, fixtures of every format, load/save round trips in a
//! scratch directory) at O0 and O3, then examples/imgtool end to end on
//! generated files.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("dodo-image-alloc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn describe(output: &Output) -> String {
    format!(
        "{}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn image_alloc_tests_pass_at_every_optimization_level() {
    let scratch = Scratch::new("tests");
    for optimization in ["0", "3"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["test", "tests/image/alloc", "-O", optimization])
            .env("IMAGE_ALLOC_TEST_DIR", &scratch.0)
            .current_dir(root())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "dodo test tests/image/alloc -O{optimization}: {}",
            describe(&output)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(" 0 failed"),
            "{}",
            describe(&output)
        );
    }
    // The round trips wrote their files into the scratch directory.
    for name in [
        "round.png",
        "round.qoi",
        "round.bmp",
        "round.pam",
        "round.jxl",
    ] {
        assert!(scratch.path(name).is_file(), "{name} missing");
    }
}

/// A width x height RGBA PAM with a smooth pattern and varied alpha, and
/// its raw samples.
fn pattern_pam(width: u32, height: u32) -> (Vec<u8>, Vec<u8>) {
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            pixels.push((x * 255 / width.max(2)) as u8);
            pixels.push((y * 255 / height.max(2)) as u8);
            pixels.push(((x * 7 + y * 3) % 256) as u8);
            pixels.push((255 - (x ^ y) % 64) as u8);
        }
    }
    let mut file = format!(
        "P7\nWIDTH {width}\nHEIGHT {height}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n"
    )
    .into_bytes();
    file.extend_from_slice(&pixels);
    (file, pixels)
}

struct Tool {
    executable: PathBuf,
    directory: PathBuf,
}

impl Tool {
    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(&self.executable)
            .args(arguments)
            .current_dir(&self.directory)
            .output()
            .unwrap()
    }
    fn ok(&self, arguments: &[&str]) -> String {
        let output = self.run(arguments);
        assert_eq!(
            output.status.code(),
            Some(0),
            "imgtool {arguments:?}: {}",
            describe(&output)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

/// The samples after the PAM header written by `imgtool convert X.pam`.
fn pam_samples(path: &Path) -> Vec<u8> {
    let data = fs::read(path).unwrap();
    let marker = b"ENDHDR\n";
    let start = data
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("PAM header")
        + marker.len();
    data[start..].to_vec()
}

#[test]
fn imgtool_converts_transforms_and_reports() {
    let scratch = Scratch::new("imgtool");
    let executable = scratch.path(&format!("imgtool{}", std::env::consts::EXE_SUFFIX));
    let built = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(["build", "examples/imgtool", "-o"])
        .arg(&executable)
        .current_dir(root())
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "build imgtool: {}",
        describe(&built)
    );
    let tool = Tool {
        executable,
        directory: scratch.0.clone(),
    };

    let (pam, pixels) = pattern_pam(61, 37);
    fs::write(scratch.path("source.pam"), &pam).unwrap();

    // PAM -> PNG -> JPEG XL -> PNG -> PAM keeps every sample.
    tool.ok(&["convert", "source.pam", "a.png"]);
    tool.ok(&["convert", "a.png", "b.jxl", "--effort", "7"]);
    tool.ok(&["convert", "b.jxl", "c.png"]);
    tool.ok(&["convert", "c.png", "d.pam"]);
    assert_eq!(&fs::read(scratch.path("a.png")).unwrap()[1..4], b"PNG");
    assert_eq!(
        &fs::read(scratch.path("b.jxl")).unwrap()[..2],
        &[0xff, 0x0a],
        "a bare JPEG XL codestream"
    );
    assert_eq!(pam_samples(&scratch.path("d.pam")), pixels);
    let compared = tool.ok(&["compare", "a.png", "c.png"]);
    assert!(compared.contains("identical: yes"), "{compared}");
    assert!(compared.contains("psnr: inf"), "{compared}");
    assert!(compared.contains("ssim: 1.000000"), "{compared}");

    // info reports the header fields.
    let info = tool.ok(&["info", "b.jxl"]);
    for expected in [
        "format: jxl",
        "size: 61x37",
        "layout: rgba8",
        "bit depth: 8",
        "color: sRGB",
        "orientation: identity (EXIF 1)",
        "frames: 1",
        "lossy: no",
        "metadata: none",
    ] {
        assert!(info.contains(expected), "missing {expected:?} in\n{info}");
    }

    // A layout change: gray, 16-bit.
    tool.ok(&["convert", "a.png", "gray.png", "--layout", "gray16"]);
    let gray = tool.ok(&["info", "gray.png"]);
    assert!(gray.contains("layout: gray16"), "{gray}");
    assert!(gray.contains("bit depth: 16"), "{gray}");

    // Resizing, with the aspect ratio kept for a zero side.
    tool.ok(&["resize", "a.png", "small.qoi", "30x20", "--filter", "box"]);
    assert!(tool.ok(&["info", "small.qoi"]).contains("size: 30x20"));
    tool.ok(&["resize", "a.png", "wide.png", "122x0", "--linear"]);
    assert!(tool.ok(&["info", "wide.png"]).contains("size: 122x74"));

    // Quarter turns, flips and crops.
    tool.ok(&["rotate", "a.png", "r90.png", "90"]);
    assert!(tool.ok(&["info", "r90.png"]).contains("size: 37x61"));
    tool.ok(&["rotate", "r90.png", "r360.png", "270"]);
    assert!(
        tool.ok(&["compare", "a.png", "r360.png"])
            .contains("identical: yes")
    );
    tool.ok(&["flip", "a.png", "h.bmp", "h"]);
    tool.ok(&["flip", "h.bmp", "hh.png", "h"]);
    assert!(
        tool.ok(&["compare", "a.png", "hh.png"])
            .contains("identical: yes")
    );
    let different = tool.run(&["compare", "a.png", "h.bmp"]);
    assert_eq!(different.status.code(), Some(3), "{}", describe(&different));
    assert!(String::from_utf8_lossy(&different.stdout).contains("identical: no"));
    tool.ok(&["crop", "source.pam", "crop.pam", "3", "4", "10", "5"]);
    let cropped = pam_samples(&scratch.path("crop.pam"));
    assert_eq!(cropped.len(), 10 * 5 * 4);
    for y in 0..5usize {
        let row = (4 + y) * 61 * 4 + 3 * 4;
        assert_eq!(&cropped[y * 40..y * 40 + 40], &pixels[row..row + 40]);
    }

    // Quantizing writes an indexed PNG (IHDR color type 3).
    let quantized = tool.ok(&["quantize", "a.png", "q.png", "16", "--dither", "ordered"]);
    assert!(quantized.contains("colors"), "{quantized}");
    assert_eq!(fs::read(scratch.path("q.png")).unwrap()[25], 3);

    // EXIF orientation: orient applies it, matching a plain 180 turn.
    let fixtures = root().join("tests/image/codecs/fixtures");
    let turned = fixtures.join("png_exif_before.png");
    let turned = turned.to_str().unwrap();
    let before = tool.ok(&["info", turned]);
    assert!(
        before.contains("orientation: rotate 180 (EXIF 3)"),
        "{before}"
    );
    assert!(
        before.contains("metadata: exif, 26 bytes (raw)"),
        "{before}"
    );
    tool.ok(&["orient", turned, "oriented.png"]);
    let after = tool.ok(&["info", "oriented.png"]);
    assert!(after.contains("orientation: identity (EXIF 1)"), "{after}");
    assert!(after.contains("metadata: exif"), "{after}");
    tool.ok(&["rotate", turned, "rotated.png", "180"]);
    assert!(
        tool.ok(&["compare", "oriented.png", "rotated.png"])
            .contains("identical: yes")
    );

    // strip drops the ICC profile; convert keeps it.
    let profiled = fixtures.join("png_color_iccp.png");
    let profiled = profiled.to_str().unwrap();
    tool.ok(&["convert", profiled, "kept.png"]);
    assert!(tool.ok(&["info", "kept.png"]).contains("metadata: icc"));
    tool.ok(&["strip", profiled, "stripped.png"]);
    assert!(
        tool.ok(&["info", "stripped.png"])
            .contains("metadata: none")
    );

    // Animation frames.
    let animation = root().join("tests/image/jxl/anim.jxl");
    let frames = tool.ok(&["frames", animation.to_str().unwrap(), "frame"]);
    assert!(frames.contains("frame-000.png"), "{frames}");
    for index in 0..3 {
        let name = format!("frame-{index:03}.png");
        assert!(scratch.path(&name).is_file(), "{name} missing");
        let expected = root().join(format!("tests/image/jxl/anim-{index}.png"));
        let compared = tool.run(&["compare", &name, expected.to_str().unwrap()]);
        assert!(
            matches!(compared.status.code(), Some(0) | Some(3)),
            "{}",
            describe(&compared)
        );
    }
    assert!(
        tool.ok(&["info", animation.to_str().unwrap()])
            .contains("frames: 3 (animated")
    );

    // Errors: a missing file, an unknown output format, bad arguments and
    // usage mistakes.
    let missing = tool.run(&["info", "missing.png"]);
    assert_eq!(missing.status.code(), Some(1), "{}", describe(&missing));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not found"));
    let unknown = tool.run(&["convert", "a.png", "out.gif"]);
    assert_eq!(unknown.status.code(), Some(1), "{}", describe(&unknown));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("output format"));
    let angle = tool.run(&["rotate", "a.png", "x.png", "45"]);
    assert_eq!(angle.status.code(), Some(1), "{}", describe(&angle));
    let garbage = scratch.path("garbage.png");
    fs::write(&garbage, b"not an image at all").unwrap();
    let undecodable = tool.run(&["convert", "garbage.png", "x.png"]);
    assert_eq!(
        undecodable.status.code(),
        Some(1),
        "{}",
        describe(&undecodable)
    );
    assert!(String::from_utf8_lossy(&undecodable.stderr).contains("unsupported"));
    let outside = tool.run(&["crop", "a.png", "x.png", "60", "0", "5", "5"]);
    assert_eq!(outside.status.code(), Some(1), "{}", describe(&outside));
    for arguments in [
        &[][..],
        &["bogus"][..],
        &["info"][..],
        &["rotate", "a.png", "b.png"][..],
    ] {
        let output = tool.run(arguments);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{arguments:?}: {}",
            describe(&output)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: imgtool"));
    }
}
