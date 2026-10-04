//! Firmware platforms: board and chip settings, `std/embedded/board` and `std/embedded/chip`
//! resolution, the embedded chip runtimes, and (when ld.lld is installed)
//! linking flashable images. Running on a real board is scripts/test_pico.py.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn dodo(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).into_owned()
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("dodo-firmware-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const BLINK: &str = "package main\nimport \"std/embedded/board\"\nfn main() {\n b := board.take()!\n for {\n  b.led.toggle()\n  b.timer.delay_ms(500)\n }\n}\n";

#[test]
fn platforms_fill_settings_that_are_not_explicit() {
    let blink = root().join("examples/blink");
    let config = stdout(&dodo(&["build", blink.to_str().unwrap(), "--print-config"]));
    for line in [
        "triple = \"thumbv6m-none-eabi\" # board pico",
        "cpu = \"cortex-m0plus\" # board pico",
        "linker = \"ld.lld\" # board pico",
        "panic-hook = \"dodo_board_panic\" # board pico",
        "board = \"pico\" # targets.blink",
        "blink.elf\" # resolved output",
    ] {
        assert!(config.contains(line), "missing {line:?} in\n{config}");
    }

    // Explicit settings win over the platform.
    let config = stdout(&dodo(&[
        "build",
        blink.to_str().unwrap(),
        "--print-config",
        "--panic",
        "trap",
        "--linker",
        "rust-lld",
    ]));
    assert!(config.contains("panic = \"trap\" # CLI"), "{config}");
    assert!(config.contains("linker = \"rust-lld\" # CLI"), "{config}");

    // A chip alone selects the same code generation for a custom board.
    let scratch = Scratch::new("chip-config");
    let source = scratch.write("main.dodo", "package main\nfn main() {}\n");
    let config = stdout(&dodo(&[
        "build",
        source.to_str().unwrap(),
        "--chip",
        "rp2040",
        "--print-config",
    ]));
    assert!(
        config.contains("triple = \"thumbv6m-none-eabi\" # chip rp2040"),
        "{config}"
    );
    assert!(config.contains("chip = \"rp2040\" # CLI"), "{config}");
}

#[test]
fn platform_selection_errors_name_the_problem() {
    let scratch = Scratch::new("errors");
    let source = scratch.write("main.dodo", "package main\nfn main() {}\n");
    let source = source.to_str().unwrap();
    let error = stderr(&dodo(&[
        "build",
        source,
        "--board",
        "pico",
        "--target",
        "x86_64-unknown-linux-gnu",
    ]));
    assert!(
        error.contains("board pico requires target thumbv6m-none-eabi, but CLI sets triple"),
        "{error}"
    );
    let error = stderr(&dodo(&["build", source, "--board", "pcio"]));
    assert!(
        error.contains("unknown board 'pcio'; did you mean 'pico'?"),
        "{error}"
    );
    let error = stderr(&dodo(&["build", source, "--chip", "rp2041"]));
    assert!(
        error.contains("unknown chip 'rp2041'; did you mean 'rp2040'?"),
        "{error}"
    );
    scratch.write(
        "dodo.toml",
        "schema = 1\n[targets.app]\nentry = \"main.dodo\"\nboard = \"arduino\"\n",
    );
    let error = stderr(&dodo(&["build", scratch.0.to_str().unwrap()]));
    assert!(error.contains("unknown board 'arduino'"), "{error}");
}

#[test]
fn std_board_and_std_chip_follow_the_selected_platform() {
    let scratch = Scratch::new("imports");
    let blink = scratch.write("blink.dodo", BLINK);
    let blink = blink.to_str().unwrap();
    stdout(&dodo(&["check", blink, "--board", "pico"]));
    let error = stderr(&dodo(&["check", blink]));
    assert!(
        error.contains("`std/embedded/board` needs a firmware board; set board = \"NAME\""),
        "{error}"
    );
    let error = stderr(&dodo(&["check", blink, "--chip", "rp2040"]));
    assert!(
        error.contains(
            "`std/embedded/board` needs a board, but this build selects only chip 'rp2040'"
        ),
        "{error}"
    );

    // A custom board configures its chip directly.
    let custom = scratch.write(
        "custom.dodo",
        "package main\nimport \"std/embedded/chip\"\nfn main() {\n p := chip.take(chip.Config { crystal_hz: 12_000_000 })!\n led := p.pins.output(15)!\n led.set_high()\n}\n",
    );
    stdout(&dodo(&[
        "check",
        custom.to_str().unwrap(),
        "--chip",
        "rp2040",
    ]));
    stdout(&dodo(&[
        "check",
        custom.to_str().unwrap(),
        "--board",
        "pico",
    ]));

    // Chip and board packages are ordinary imports too, and they check on a
    // desktop, so code using their types can share a project with host tests.
    let explicit = scratch.write(
        "explicit.dodo",
        "package main\nimport \"std/embedded/board/pico\"\nimport \"std/embedded/chip/rp2040\"\nfn blink(led: &mut rp2040.Pin) {\n led.toggle()\n}\nfn main() {\n b := pico.take()!\n blink(&mut b.led)\n}\n",
    );
    stdout(&dodo(&["check", explicit.to_str().unwrap()]));
}

#[test]
fn chip_runtimes_provide_startup_and_helper_symbols() {
    let scratch = Scratch::new("runtime");
    for chip in dodoc::hardware::CHIPS {
        let object = scratch.0.join(format!("{}.o", chip.name));
        let options = dodoc::codegen::Options {
            target: Some(chip.triple.into()),
            cpu: Some(chip.cpu.into()),
            optimization: 2,
            ..Default::default()
        };
        dodoc::codegen::compile_ir_object(&options, chip.name, chip.runtime, &object).unwrap();
        let bytes = fs::read(&object).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for name in ["Reset_Handler", chip.panic_hook, "memcpy", "memset"] {
            assert!(text.contains(name), "{} runtime lacks {name}", chip.name);
        }
    }
    let rp2040 = fs::read(scratch.0.join("rp2040.o")).unwrap();
    let text = String::from_utf8_lossy(&rp2040);
    for name in [
        ".boot2",
        ".vector_table",
        "dodo_rp2040_reset_usb_boot",
        "SysTick_Handler",
        "TIMER_IRQ_0",
        "__aeabi_uldivmod",
        "__udivdi3",
        "__muldi3",
        "__ashrdi3",
    ] {
        assert!(text.contains(name), "rp2040 runtime lacks {name}");
    }
}

fn ld_lld() -> bool {
    let present = Command::new("ld.lld")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    assert!(
        present || std::env::var_os("DODO_REQUIRE_BOARD_TESTS").is_none(),
        "ld.lld is required for firmware link tests"
    );
    if !present {
        eprintln!("skipping firmware link test; install ld.lld or set DODO_REQUIRE_BOARD_TESTS=1");
    }
    present
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// CRC-32/MPEG-2, as checked by the RP2040 boot ROM.
fn crc32_mpeg2(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= (byte as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// The UF2 payload as one flash image starting at 0x10000000.
fn flash_image(uf2: &[u8]) -> Vec<u8> {
    assert_eq!(uf2.len() % 512, 0);
    let blocks = uf2.len() / 512;
    let mut image = vec![];
    for (index, block) in uf2.chunks(512).enumerate() {
        assert_eq!(word(block, 0), 0x0A32_4655);
        assert_eq!(word(block, 4), 0x9E5D_5157);
        assert_eq!(word(block, 8), 0x2000, "family ID flag");
        assert_eq!(word(block, 16), 256);
        assert_eq!(word(block, 20) as usize, index);
        assert_eq!(word(block, 24) as usize, blocks);
        assert_eq!(word(block, 28), 0xE48B_FF56, "RP2040 family");
        assert_eq!(word(block, 508), 0x0AB1_6F30);
        let offset = (word(block, 12) - 0x1000_0000) as usize;
        image.resize(image.len().max(offset + 256), 0);
        image[offset..offset + 256].copy_from_slice(&block[32..288]);
    }
    image
}

fn check_rp2040_firmware(elf: &Path) {
    let image = flash_image(&fs::read(elf.with_extension("uf2")).unwrap());
    // The boot ROM checks stage 2's CRC before running it.
    assert_eq!(word(&image, 252), crc32_mpeg2(&image[..252]));
    // Vector table: stack at the top of SRAM, Thumb reset handler = ELF entry.
    assert_eq!(word(&image, 0x100), 0x2004_2000);
    let entry = word(&fs::read(elf).unwrap(), 0x18);
    assert_eq!(word(&image, 0x104), entry);
    assert_eq!(entry & 1, 1);
}

fn build(args: &[&str]) {
    let output = dodo(args);
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{args:?}: {message}");
    assert!(message.contains(".uf2"), "{message}");
}

#[test]
fn rp2040_projects_link_to_flashable_firmware() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("link");
    for (project, name) in [
        ("examples/blink", "blink"),
        ("tests/hardware/pico", "selftest"),
    ] {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("{name}-O{level}.elf"));
            let project = root().join(project);
            build(&["build", project.to_str().unwrap(), "-O", level, "-o", &elf]);
            check_rp2040_firmware(Path::new(&elf));
        }
    }
    // A custom board with only `chip` set.
    let custom = scratch.write(
        "custom.dodo",
        "package main\nimport \"std/embedded/chip\"\nfn main() {\n p := chip.take(chip.Config { crystal_hz: 12_000_000 })!\n led := p.pins.output(15)!\n led.set_high()\n}\n",
    );
    let elf = scratch.path("custom.elf");
    build(&[
        "build",
        custom.to_str().unwrap(),
        "--chip",
        "rp2040",
        "-o",
        &elf,
    ]);
    check_rp2040_firmware(Path::new(&elf));
}

#[test]
fn floating_point_link_errors_explain_the_limit() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("float");
    let source = scratch.write(
        "main.dodo",
        "package main\nimport \"std/embedded/board\"\nfn main() {\n b := board.take()!\n t := b.timer.now_us() as f64\n if t * 1.5 > 1.0 {\n  board.reboot_to_bootloader()\n }\n}\n",
    );
    let error = stderr(&dodo(&[
        "build",
        source.to_str().unwrap(),
        "--board",
        "pico",
    ]));
    assert!(
        error.contains("floating-point arithmetic is not supported in firmware yet"),
        "{error}"
    );
}
