//! std/gpu: code generation for every hosted target, the fixture and the
//! compute example on whatever GPU the machine has (Metal on macOS; Vulkan or
//! OpenGL elsewhere, Mesa's lavapipe and llvmpipe in a Linux container are
//! enough), recording mistakes that must panic, and backend selection.
//! Without a usable GPU the programs must say so and exit with status 2.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HOSTED_TARGETS: [&str; 6] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-pc-windows-gnu",
];

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("dodo-gpu-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn dodo(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(arguments)
        .output()
        .unwrap()
}

fn build(source: &Path, executable: &Path, optimization: &str) {
    success(
        &dodo(&[
            "build",
            source.to_str().unwrap(),
            "-O",
            optimization,
            "-o",
            executable.to_str().unwrap(),
        ]),
        &format!("compile {} -O{optimization}", source.display()),
    );
}

fn executable(scratch: &Scratch, name: &str) -> PathBuf {
    scratch
        .0
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

#[test]
fn gpu_builds_for_every_hosted_target() {
    let scratch = Scratch::new("targets");
    for (name, source) in [
        ("checks", root().join("tests/stdlib/gpu_checks.dodo")),
        ("compute", root().join("examples/gpu_compute.dodo")),
        ("triangle", root().join("examples/gpu_triangle.dodo")),
        ("window", root().join("examples/gpu_window.dodo")),
        (
            "surface",
            root().join("tests/stdlib/gpu_surface_checks.dodo"),
        ),
    ] {
        for target in HOSTED_TARGETS {
            let object = scratch.0.join(format!("{name}-{target}.o"));
            success(
                &dodo(&[
                    "build",
                    source.to_str().unwrap(),
                    "--emit",
                    "obj",
                    "--target",
                    target,
                    "-o",
                    object.to_str().unwrap(),
                ]),
                &format!("emit {name} for {target}"),
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
}

/// Every check on every backend the machine offers, unoptimized and at -O3.
#[test]
fn gpu_checks_run_on_every_available_backend() {
    let scratch = Scratch::new("checks");
    let source = root().join("tests/stdlib/gpu_checks.dodo");
    for optimization in ["0", "3"] {
        let program = executable(&scratch, &format!("checks-O{optimization}"));
        build(&source, &program, optimization);
        let output = Command::new(&program).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        match output.status.code() {
            Some(0) => {
                assert!(stdout.ends_with("gpu ok\n"), "{stdout}");
                if cfg!(target_os = "macos") {
                    assert!(stdout.contains("Metal ok"), "{stdout}");
                }
            }
            Some(2) => {
                assert!(stdout.ends_with("no backend\n"), "{stdout}");
                eprintln!("no GPU backend: only device-independent paths ran");
            }
            _ => panic!(
                "gpu checks -O{optimization}: {}\n{stdout}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }
}

/// The checks again with DODO_GPU_UNMAPPED, which makes the Vulkan backend
/// treat buffers as unmapped, as on a GPU with memory of its own: writes
/// then go through GPU copies and reads through the host-cached readback
/// buffer even on unified-memory GPUs. Other backends ignore it.
#[test]
fn gpu_checks_pass_with_unmapped_buffers() {
    let scratch = Scratch::new("unmapped");
    let program = executable(&scratch, "checks");
    build(&root().join("tests/stdlib/gpu_checks.dodo"), &program, "0");
    let output = Command::new(&program)
        .env("DODO_GPU_UNMAPPED", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    match output.status.code() {
        Some(0) => assert!(stdout.ends_with("gpu ok\n"), "{stdout}"),
        Some(2) => assert!(stdout.ends_with("no backend\n"), "{stdout}"),
        _ => panic!(
            "gpu checks with unmapped buffers: {}\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

#[test]
fn compute_example_runs_or_reports_a_missing_gpu() {
    let scratch = Scratch::new("compute");
    let program = executable(&scratch, "compute");
    build(&root().join("examples/gpu_compute.dodo"), &program, "0");
    let output = Command::new(&program).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    match output.status.code() {
        Some(0) => assert!(
            stdout.contains("1000 of 1000 values correct") && stdout.ends_with("compute ok\n"),
            "{stdout}"
        ),
        Some(2) => assert!(
            stdout == "NoBackend\n" || stdout == "Unsupported\n",
            "{stdout}"
        ),
        _ => panic!(
            "compute example: {}\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// A program that opens a device, prints "open", then makes one recording
/// mistake. Without a device it exits 2 before the mistake.
fn mistake(body: &str) -> String {
    format!(
        r#"package main
import "std/console"
import "std/gpu"
fn main() -> i32 {{
    device := match gpu.Device.open(gpu.Options.default()) {{
        ok(device) => device,
        err(_) => {{
            return 2
        }}
    }}
    console.println("open")!
    match run(&device) {{
        ok() => {{
            return 0
        }}
        err(_) => {{
            return 3
        }}
    }}
}}
fn run(device: &gpu.Device) -> void!gpu.Error {{
{body}
    return ok()
}}
"#
    )
}

#[test]
fn recording_mistakes_panic() {
    let scratch = Scratch::new("mistakes");
    let cases = [
        (
            "dispatch without a pipeline",
            r#"    encoder := device.encoder()?
    pass := encoder.compute_pass()
    pass.dispatch(1, 1, 1)"#,
            "no pipeline is set",
        ),
        (
            "copy past the end",
            r#"    source := device.create_buffer(16, gpu.BUFFER_COPY_SRC)?
    target := device.create_buffer(16, gpu.BUFFER_COPY_DST)?
    encoder := device.encoder()?
    encoder.copy_buffer(&source, 8, &target, 0, 16)"#,
            "the copy range lies outside the buffer",
        ),
        (
            "copy without usage",
            r#"    source := device.create_buffer(16, gpu.BUFFER_STORAGE)?
    target := device.create_buffer(16, gpu.BUFFER_COPY_DST)?
    encoder := device.encoder()?
    encoder.copy_buffer(&source, 0, &target, 0, 16)"#,
            "the copy source needs BUFFER_COPY_SRC",
        ),
        (
            "render into a texture without TEXTURE_RENDER",
            r#"    texture := device.create_texture(gpu.TextureDescriptor.new(4, 4, gpu.FORMAT_RGBA8_UNORM, gpu.TEXTURE_SAMPLED))?
    encoder := device.encoder()?
    pass := encoder.render_pass(gpu.ColorAttachment.clear(&texture, 0.0, 0.0, 0.0, 1.0))
    pass.end()"#,
            "a render pass attachment needs TEXTURE_RENDER",
        ),
        (
            "the same texture attached twice",
            r#"    texture := device.create_texture(gpu.TextureDescriptor.new(4, 4, gpu.FORMAT_RGBA8_UNORM, gpu.TEXTURE_RENDER))?
    encoder := device.encoder()?
    colors := [gpu.ColorAttachment.clear(&texture, 0.0, 0.0, 0.0, 1.0), gpu.ColorAttachment.keep(&texture)]
    pass := encoder.render_pass_targets(&colors)
    pass.end()"#,
            "a texture is attached to the pass twice",
        ),
        (
            "resolve a single-sampled attachment",
            r#"    texture := device.create_texture(gpu.TextureDescriptor.new(4, 4, gpu.FORMAT_RGBA8_UNORM, gpu.TEXTURE_RENDER))?
    target := device.create_texture(gpu.TextureDescriptor.new(4, 4, gpu.FORMAT_RGBA8_UNORM, gpu.TEXTURE_RENDER))?
    encoder := device.encoder()?
    pass := encoder.render_pass(gpu.ColorAttachment.clear(&texture, 0.0, 0.0, 0.0, 1.0).with_resolve(&target))
    pass.end()"#,
            "only multisampled attachments resolve",
        ),
        (
            "attachments with different sample counts",
            r#"    color := device.create_texture(gpu.TextureDescriptor.multisampled(4, 4, gpu.FORMAT_RGBA8_UNORM))?
    depth := device.create_texture(gpu.TextureDescriptor.new(4, 4, gpu.FORMAT_DEPTH32_FLOAT, gpu.TEXTURE_RENDER))?
    encoder := device.encoder()?
    pass := encoder.render_pass_with_depth(gpu.ColorAttachment.clear(&color, 0.0, 0.0, 0.0, 1.0), gpu.DepthAttachment.clear(&depth, 1.0))
    pass.end()"#,
            "the attachments differ in their sample counts",
        ),
        (
            "mipmaps of an integer texture",
            r#"    texture := device.create_texture(gpu.TextureDescriptor.mipmapped(4, 4, gpu.FORMAT_R32_UINT, gpu.TEXTURE_COPY_DST))?
    encoder := device.encoder()?
    encoder.generate_mipmaps(&texture)"#,
            "generating mipmaps needs a filterable color format",
        ),
        (
            "unaligned binding offset",
            r#"    buffer := device.create_buffer(1024, gpu.BUFFER_STORAGE)?
    encoder := device.encoder()?
    pass := encoder.compute_pass()
    pass.set_buffer_range(0, &buffer, 4, 16)"#,
            "bound buffer offsets are multiples of BINDING_ALIGNMENT",
        ),
    ];
    for (name, body, message) in cases {
        let source = scratch.0.join("mistake.dodo");
        fs::write(&source, mistake(body)).unwrap();
        let program = executable(&scratch, "mistake");
        build(&source, &program, "0");
        let output = Command::new(&program).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.code() == Some(2) && stdout.is_empty() {
            eprintln!("no GPU backend: skipped {name}");
            continue;
        }
        assert!(
            !output.status.success() && output.status.code() != Some(3),
            "{name}: {}\n{stdout}\n{stderr}",
            output.status
        );
        assert_eq!(stdout, "open\n", "{name}: {stderr}");
        // A failed assertion reports its line, whose message names the mistake.
        let location = stderr
            .split("<stdlib>/std/gpu.dodo:")
            .nth(1)
            .unwrap_or_else(|| panic!("{name}: no std/gpu location in {stderr}"));
        let line: usize = location.split(':').next().unwrap().parse().unwrap();
        let text = fs::read_to_string(root().join("stdlib/std/gpu.dodo")).unwrap();
        let reported = text.lines().nth(line - 1).unwrap();
        assert!(
            reported.contains(message),
            "{name}: line {line} is {reported:?}"
        );
    }
}

#[test]
fn native_backend_follows_the_target() {
    let scratch = Scratch::new("native");
    let program = scratch.0.join("native.dodo");
    for (import, target, accepted) in [
        ("std/gpu/hal/native", "aarch64-apple-darwin", true),
        ("std/gpu/hal/native", "x86_64-unknown-linux-gnu", true),
        ("std/gpu/hal/native", "x86_64-pc-windows-msvc", true),
        ("std/gpu/hal/macos", "x86_64-apple-darwin", true),
        ("std/gpu/hal/macos", "aarch64-unknown-linux-gnu", false),
        ("std/gpu/hal/linux", "x86_64-pc-windows-gnu", false),
        ("std/gpu", "wasm32-unknown-unknown", false),
    ] {
        fs::write(
            &program,
            format!("package main\nimport \"{import}\" as gpu\nfn main() {{}}\n"),
        )
        .unwrap();
        let output = dodo(&["check", program.to_str().unwrap(), "--target", target]);
        assert_eq!(
            output.status.success(),
            accepted,
            "{import} for {target}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
