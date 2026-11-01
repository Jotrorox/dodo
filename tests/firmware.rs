//! Firmware platforms: board and chip settings, `std/embedded/board` and `std/embedded/chip`
//! resolution, the embedded chip runtimes, and (when ld.lld is installed)
//! linking flashable images. Running on a real board is scripts/test_pico.py
//! (`--board pico`, `--board pico2`, or `--board pico2_w`).
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
fn uart_ports_compile_for_both_chips() {
    let scratch = Scratch::new("uart");
    let source = scratch.write(
        "uart.dodo",
        r#"package main
import "std/embedded/board"
import "std/io"
fn main() {
    b := board.take()!
    first := b.pins.uart(0, some(1u32), 115_200)!
    second := b.pins.uart(4, some(5u32), 9600)!
    first.set_poll_limit(1000)
    second.set_read_poll_limit(1000)
    data := [42u8; 3]
    io.write_all(&mut first, &data)!
    first.flush()!
    io.read_exact(&mut second, &mut data)!
    b.pins.release_uart(first)
    b.pins.release_uart(second)
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        let object = scratch.path(&format!("uart-{board}.o"));
        stdout(&dodo(&[
            "build",
            source.to_str().unwrap(),
            "--board",
            board,
            "--emit",
            "obj",
            "-o",
            &object,
        ]));
    }
}

// The board watchdog: started, fed through the portable protocol, its reset
// reason and scratch words read, and a deliberate restart. Needs no linker.
#[test]
fn watchdogs_compile_for_every_board() {
    let scratch = Scratch::new("watchdog");
    let source = scratch.write(
        "watchdog.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/hal"
fn keep_alive<W: hal.Watchdog>(dog: &mut W) {
    dog.feed()
}
fn main() {
    b := board.take()!
    if b.watchdog.reset_reason() == hal.ResetReason.Timeout {
        count := b.watchdog.scratch(0)
        b.watchdog.set_scratch(0, count + 1)
    }
    longest := b.watchdog.max_timeout_ms()
    b.watchdog.start(longest)!
    for _ in 0..10u32 {
        b.timer.delay_ms(100)
        keep_alive(&mut b.watchdog)
    }
    if b.watchdog.remaining_us() < 1000 && b.watchdog.is_running() {
        b.watchdog.stop()
    }
    b.watchdog.restart()
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        for level in ["0", "2"] {
            let object = scratch.path(&format!("watchdog-{board}-O{level}.o"));
            stdout(&dodo(&[
                "build",
                source.to_str().unwrap(),
                "--board",
                board,
                "-O",
                level,
                "--emit",
                "obj",
                "-o",
                &object,
            ]));
        }
    }
}

// The executor on a board: tasks woken by an alarm, a GPIO edge, and the
// UART, with the alarm as the executor's clock. Needs no linker.
#[test]
fn executor_tasks_compile_for_every_board() {
    let scratch = Scratch::new("executor");
    let tasks = root().join("examples/tasks");
    let source = scratch.write(
        "clock.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip"
import "std/embedded/executor"
struct Once {
    timer: executor.Timer
    started: bool
    pub fn poll(&mut self, cx: &mut executor.Context) -> executor.Poll {
        if !self.started {
            self.started = true
            self.timer.start(cx, 1500)
        }
        if self.timer.expired(cx) {
            return executor.Poll.Ready
        }
        return executor.Poll.Pending
    }
}
fn main() {
    b := board.take()!
    pin := b.pins.input(15)!
    pin.listen(chip.FALLING_EDGE | chip.LEVEL_HIGH)
    core.assert(pin.events() <= 0xF)
    pin.clear_events(chip.FALLING_EDGE)
    pin.ignore(chip.LEVEL_HIGH)
    core.assert(pin.irq() == chip.IO_IRQ_BANK0)
    b.pins.release(pin)
    alarm := b.timer.alarm(2)!
    soon := alarm.now_us() + 10
    alarm.sleep_until(some(soon))
    once := Once { timer: executor.Timer.new(), started: false }
    tasks: [1]&mut dyn executor.Task = [&mut once]
    runner := executor.Executor.new()
    end := alarm.now_us() + 10_000
    core.assert(runner.run_until(&mut tasks, &mut alarm, end))
    b.timer.release_alarm(alarm)
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        for (input, name) in [(tasks.as_path(), "tasks"), (source.as_path(), "clock")] {
            for level in ["0", "2"] {
                let object = scratch.path(&format!("{name}-{board}-O{level}.o"));
                stdout(&dodo(&[
                    "build",
                    input.to_str().unwrap(),
                    "--board",
                    board,
                    "-O",
                    level,
                    "--emit",
                    "obj",
                    "-o",
                    &object,
                ]));
            }
        }
    }
}

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

// rp_pio.assemble assembles PIO programs at compile time, for the RP2040
// and RP2350 only, checking each program against its chip's PIO version.
#[test]
fn pio_programs_assemble_only_for_rp_chips() {
    let scratch = Scratch::new("pio");
    let program = |lines: &str| {
        format!(
            "package main\nimport \"std/embedded/chip/rp_pio\"\nconst PROGRAM: rp_pio.Program = rp_pio.assemble(\n{lines}\n)\nfn main() {{\n core.assert(PROGRAM.length > 0)\n}}\n"
        )
    };
    let valid = scratch.write(
        "valid.dodo",
        &program("    \".side_set 1 opt\",\n    \"loop:\",\n    \"    out pins, 1 side 0 [2]\",\n    \"    jmp loop    side 1\","),
    );
    let valid = valid.to_str().unwrap();
    for board in ["pico", "pico2", "pico2_w"] {
        stdout(&dodo(&["check", valid, "--board", board]));
    }
    stdout(&dodo(&["check", valid, "--chip", "rp2350"]));
    // Hosted builds have no PIO.
    let error = stderr(&dodo(&["check", valid]));
    assert!(
        error.contains("PIO programs are assembled for the RP2040 and RP2350 only")
            && error.contains("valid.dodo:3:33"),
        "{error}"
    );
    // An error points into its line.
    let invalid = scratch.write(
        "invalid.dodo",
        &program("    \"set x, 1\",\n    \"    jmp nowhere\","),
    );
    let error = stderr(&dodo(&[
        "check",
        invalid.to_str().unwrap(),
        "--board",
        "pico2",
    ]));
    assert!(
        error.contains("invalid PIO program: undefined symbol `nowhere`")
            && error.contains("invalid.dodo:5:14"),
        "{error}"
    );
    // RP2350 instructions are refused on the RP2040.
    let newer = scratch.write("newer.dodo", &program("    \"mov pindirs, ~null\","));
    let newer = newer.to_str().unwrap();
    stdout(&dodo(&["check", newer, "--board", "pico2"]));
    let error = stderr(&dodo(&["check", newer, "--board", "pico"]));
    assert!(
        error.contains("`mov pindirs` needs PIO version 1 (the RP2350), but the build's chip, the RP2040, has PIO version 0"),
        "{error}"
    );
    // Only literals, and only rp_pio may declare the assembler.
    let runtime = scratch.write(
        "runtime.dodo",
        "package main\nimport \"std/embedded/chip/rp_pio\"\nfn main() {\n line := \"nop\"\n program := rp_pio.assemble(line)\n}\n",
    );
    let error = stderr(&dodo(&[
        "check",
        runtime.to_str().unwrap(),
        "--board",
        "pico",
    ]));
    assert!(
        error.contains("rp_pio.assemble takes string literals"),
        "{error}"
    );
    let reserved = scratch.write(
        "reserved.dodo",
        "package main\n@compiler(pio)\npub fn assemble(source: &str) -> u32\nfn main() {}\n",
    );
    let error = stderr(&dodo(&[
        "check",
        reserved.to_str().unwrap(),
        "--board",
        "pico",
    ]));
    assert!(
        error.contains("@compiler(pio) is reserved for std/embedded/chip/rp_pio"),
        "{error}"
    );
}

// The WS2812 example and the rest of the PIO driver on every board. Needs
// no linker.
#[test]
fn pio_drivers_compile_for_every_board() {
    let scratch = Scratch::new("pio-compile");
    let example = root().join("examples/ws2812.dodo");
    let source = scratch.write(
        "pio.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip/rp_pio"
import "std/embedded/cortex_m"
const ECHO: rp_pio.Program = rp_pio.assemble(".in 32 left auto 8", "    pull block", "    mov isr, ::osr", "    push iffull noblock")
fn main() {
    b := board.take()!
    pio := b.pins.pio(1)!
    b.pins.pio_pins(&pio, 6, 2)!
    loaded := pio.load(&ECHO)!
    config := loaded.config()
    config.set_out_pins(6, 2)!
    config.set_set_pins(6, 2)!
    config.set_in_pins(8)!
    config.set_jmp_pin(9)!
    config.set_out_special(true, none)!
    config.set_mov_status(rp_pio.STATUS_TX_LESS, 1)!
    config.set_frequency(10_000_000)!
    sm := pio.free_state_machine()!
    sm.init(&config)
    sm.set_pindirs(6, 2, 0b11)
    pio.listen(0, rp_pio.rx_not_empty(sm.index()) | rp_pio.flag(0))
    cortex_m.enable(pio.irq(0))
    pio.enable_in_sync(1u32 << sm.index())
    sm.put(5)
    if let some(word) = sm.try_get() {
        sm.exec_wait(ECHO.instructions[0])
        sm.set_frequency(word + 1)!
    }
    sm.drain_tx()
    flags := pio.flags()
    pio.clear_flags(flags)
    pio.release_state_machine(sm)
    pio.unload(loaded)
    b.pins.release_pio(pio)
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        for (name, path) in [("ws2812", &example), ("pio", &source)] {
            for level in ["0", "2"] {
                let object = scratch.path(&format!("{name}-{board}-O{level}.o"));
                stdout(&dodo(&[
                    "build",
                    path.to_str().unwrap(),
                    "--board",
                    board,
                    "-O",
                    level,
                    "--emit",
                    "obj",
                    "-o",
                    &object,
                ]));
            }
        }
    }
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
    let explicit = scratch.write(
        "explicit2.dodo",
        "package main\nimport \"std/embedded/board/pico2\"\nimport \"std/embedded/chip/rp2350\"\nfn blink(led: &mut rp2350.Pin) {\n led.toggle()\n}\nfn main() {\n b := pico2.take()!\n blink(&mut b.led)\n}\n",
    );
    stdout(&dodo(&["check", explicit.to_str().unwrap()]));
    stdout(&dodo(&["check", blink, "--board", "pico2"]));
    // The Pico 2 W's LED is behind the wireless chip, but it is still an
    // output pin to portable code.
    let explicit = scratch.write(
        "explicit2w.dodo",
        "package main\nimport \"std/embedded/board/pico2_w\"\nimport \"std/embedded/hal\"\nfn main() {\n b := pico2_w.take()!\n hal.write_pin(&mut b.led, true)\n b.led.toggle()\n}\n",
    );
    stdout(&dodo(&["check", explicit.to_str().unwrap()]));
    stdout(&dodo(&["check", blink, "--board", "pico2_w"]));
    stdout(&dodo(&[
        "check",
        custom.to_str().unwrap(),
        "--chip",
        "rp2350",
    ]));
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
        for name in [
            "Reset_Handler",
            chip.panic_hook,
            "dodo_panic_report",
            "USBCTRL_IRQ",
            "dodo_usb_interrupt",
            "DMA_IRQ_0",
            "dodo_dma_interrupt",
            "FifoHandler",
            "dodo_fifo_interrupt",
            "dodo_core_id",
            "dodo_core1_trampoline",
            "dodo_core1_entry",
            "dodo_flash_erase",
            "dodo_flash_program",
            "dodo_flash_image_end",
            "dodo_flash_end",
            "memcpy",
            "memset",
        ] {
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
        "__aeabi_fadd",
        "__aeabi_dmul",
        "__adddf3",
        "__aeabi_f2d",
        "fmodf",
    ] {
        assert!(text.contains(name), "rp2040 runtime lacks {name}");
    }
    let rp2350 = fs::read(scratch.0.join("rp2350.o")).unwrap();
    let text = String::from_utf8_lossy(&rp2350);
    for name in [
        ".picobin_block",
        ".vector_table",
        "dodo_rp2350_rom_call",
        "SecureFault_Handler",
        "TIMER0_IRQ_0",
        "POWMAN_IRQ_TIMER",
        "__aeabi_uldivmod",
        "__udivdi3",
        "__aeabi_ddiv",
        "__divdf3",
        "__aeabi_f2d",
        "__aeabi_l2f",
        "dodo_dcp_save",
        "dodo_soft_ddiv",
        "fmod",
    ] {
        assert!(text.contains(name), "rp2350 runtime lacks {name}");
    }
}

#[test]
fn flash_storage_links_for_every_board() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("storage");
    let example = root().join("examples/boot_counter.dodo");
    for (board, flash_size, family) in [
        ("pico", 2u32 << 20, 0xE48B_FF56),
        ("pico2", 4 << 20, 0xE48B_FF59),
        ("pico2_w", 4 << 20, 0xE48B_FF59),
    ] {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("boot-counter-{board}-O{level}.elf"));
            build(&[
                "build",
                example.to_str().unwrap(),
                "--board",
                board,
                "-O",
                level,
                "-o",
                &elf,
            ]);
            let path = Path::new(&elf);
            if board == "pico" {
                check_rp2040_firmware(path);
            } else {
                check_rp2350_firmware(path);
            }
            let bytes = fs::read(path).unwrap();
            let symbol = |name: &str| {
                elf_symbol(&bytes, name).unwrap_or_else(|| panic!("{board} lacks {name}"))
            };
            // rp_flash sizes the flash from the board and keeps regions above
            // the image, which ends where the UF2 payload does.
            assert_eq!(symbol("__flash_end"), 0x1000_0000 + flash_size);
            let image = flash_image(&fs::read(path.with_extension("uf2")).unwrap(), family);
            let end = (symbol("__flash_image_end") - 0x1000_0000) as usize;
            assert_eq!(end.div_ceil(256) * 256, image.len(), "{board}");
            let in_sram = |address: u32| (0x2000_0000..0x2008_2000).contains(&address);
            if board == "pico" {
                // With execute-in-place off, the RP2040 routine and the copy
                // of .boot2 it restores it with run from SRAM.
                assert!(in_sram(symbol("dodo_flash_erase")));
                assert!(in_sram(symbol("dodo_flash_program")));
                assert!(in_sram(symbol("dodo_rp2040_boot2_copy")));
            } else {
                // The RP2350's flash_op restores execute-in-place itself.
                assert!(symbol("dodo_flash_erase") >= 0x1000_0000);
            }
        }
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
fn flash_image(uf2: &[u8], family: u32) -> Vec<u8> {
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
        assert_eq!(word(block, 28), family, "UF2 family");
        assert_eq!(word(block, 508), 0x0AB1_6F30);
        let offset = (word(block, 12) - 0x1000_0000) as usize;
        image.resize(image.len().max(offset + 256), 0);
        image[offset..offset + 256].copy_from_slice(&block[32..288]);
    }
    image
}

fn check_rp2040_firmware(elf: &Path) {
    let image = flash_image(&fs::read(elf.with_extension("uf2")).unwrap(), 0xE48B_FF56);
    // The boot ROM checks stage 2's CRC before running it.
    assert_eq!(word(&image, 252), crc32_mpeg2(&image[..252]));
    // Vector table: stack at the top of SRAM, Thumb reset handler = ELF entry.
    assert_eq!(word(&image, 0x100), 0x2004_2000);
    let entry = word(&fs::read(elf).unwrap(), 0x18);
    assert_eq!(word(&image, 0x104), entry);
    assert_eq!(entry & 1, 1);
}

fn check_rp2350_firmware(elf: &Path) {
    let image = flash_image(&fs::read(elf.with_extension("uf2")).unwrap(), 0xE48B_FF59);
    // The boot ROM enters through the vector table at the start of flash:
    // stack at the top of SRAM, Thumb reset handler = ELF entry.
    assert_eq!(word(&image, 0), 0x2008_2000);
    let entry = word(&fs::read(elf).unwrap(), 0x18);
    assert_eq!(word(&image, 4), entry);
    assert_eq!(entry & 1, 1);
    // A secure Arm RP2350 IMAGE_DEF block in the first 4 KiB.
    let block = (0..4096)
        .step_by(4)
        .find(|&at| word(&image, at) == 0xFFFF_DED3)
        .expect("IMAGE_DEF block");
    assert_eq!(word(&image, block + 4), 0x1021_0142);
    assert_eq!(word(&image, block + 8), 0x0000_01FF);
    assert_eq!(word(&image, block + 16), 0xAB12_3579);
}

/// The value of the ELF32 symbol `name`, if the file defines it.
fn elf_symbol(elf: &[u8], name: &str) -> Option<u32> {
    let half = |at: usize| u16::from_le_bytes(elf[at..at + 2].try_into().unwrap()) as usize;
    let header = |index: usize| word(elf, 0x20) as usize + index * half(0x2E);
    let symbols = (0..half(0x30))
        .map(header)
        .find(|&at| word(elf, at + 4) == 2)?;
    let names = word(elf, header(word(elf, symbols + 24) as usize) + 16) as usize;
    let start = word(elf, symbols + 16) as usize;
    let end = start + word(elf, symbols + 20) as usize;
    (start..end).step_by(16).find_map(|at| {
        let offset = names + word(elf, at) as usize;
        let length = elf[offset..].iter().position(|&b| b == 0)?;
        let defined = half(at + 14) != 0;
        (defined && &elf[offset..offset + length] == name.as_bytes()).then(|| word(elf, at + 4))
    })
}

/// The address word in the literal pool of the runtime's panic hook, which
/// it calls if nonzero: the reporter's address or 0.
fn panic_hook_literal(elf: &Path, family: u32) -> u32 {
    let bytes = fs::read(elf).unwrap();
    let image = flash_image(&fs::read(elf.with_extension("uf2")).unwrap(), family);
    let hook = (elf_symbol(&bytes, "dodo_board_panic").expect("panic hook") & !1) as usize;
    // cpsid, ldr, cmp, beq, blx, b: six halfwords, then the aligned word.
    let at = (hook - 0x1000_0000 + 12 + 3) & !3;
    word(&image, at)
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
        ("examples/tasks", "tasks"),
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
    // Portable drivers, tested with fakes, on the board's I2C bus and on
    // an ADC input and a PWM output, WS2812 LEDs on PIO, an ST7789 panel
    // on SPI, and an SSD1306 panel on I2C.
    for name in [
        "hal_sensor_board",
        "dimmer_board",
        "ws2812",
        "thermometer_board",
        "oled_demo",
    ] {
        let source = root().join(format!("examples/{name}.dodo"));
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("{name}-O{level}.elf"));
            build(&[
                "build",
                source.to_str().unwrap(),
                "--board",
                "pico",
                "-O",
                level,
                "-o",
                &elf,
            ]);
            check_rp2040_firmware(Path::new(&elf));
        }
    }
}

#[test]
fn rp2350_projects_link_to_flashable_firmware() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("link-rp2350");
    let blink = root().join("examples/blink");
    let sensor = root().join("examples/hal_sensor_board.dodo");
    let dimmer = root().join("examples/dimmer_board.dodo");
    let ws2812 = root().join("examples/ws2812.dodo");
    let thermometer = root().join("examples/thermometer_board.dodo");
    let oled = root().join("examples/oled_demo.dodo");
    let selftest = root().join("tests/hardware/pico2");
    let selftest_w = root().join("tests/hardware/pico2_w");
    let float = root().join("tests/hardware/rp2350_float");
    let tasks = root().join("examples/tasks");
    for (project, name, board) in [
        (&blink, "blink", &["--board", "pico2"][..]),
        (&blink, "blink-w", &["--board", "pico2_w"][..]),
        (&tasks, "tasks", &["--board", "pico2"][..]),
        (&tasks, "tasks-w", &["--board", "pico2_w"][..]),
        (&sensor, "sensor", &["--board", "pico2"][..]),
        (&sensor, "sensor-w", &["--board", "pico2_w"][..]),
        (&dimmer, "dimmer", &["--board", "pico2"][..]),
        (&dimmer, "dimmer-w", &["--board", "pico2_w"][..]),
        (&ws2812, "ws2812", &["--board", "pico2"][..]),
        (&ws2812, "ws2812-w", &["--board", "pico2_w"][..]),
        (&thermometer, "thermometer", &["--board", "pico2"][..]),
        (&thermometer, "thermometer-w", &["--board", "pico2_w"][..]),
        (&oled, "oled", &["--board", "pico2"][..]),
        (&oled, "oled-w", &["--board", "pico2_w"][..]),
        (&selftest, "selftest", &[][..]),
        (&selftest_w, "selftest-w", &[][..]),
        (&float, "float", &["-b", "pico2"][..]),
        (&float, "float-w", &["-b", "pico2_w"][..]),
    ] {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("{name}-O{level}.elf"));
            let mut args = vec!["build", project.to_str().unwrap(), "-O", level, "-o", &elf];
            args.extend(board);
            build(&args);
            check_rp2350_firmware(Path::new(&elf));
        }
    }
    let custom = scratch.write(
        "custom.dodo",
        "package main\nimport \"std/embedded/chip\"\nfn main() {\n p := chip.take(chip.Config { crystal_hz: 12_000_000 })!\n led := p.pins.output(15)!\n led.set_high()\n}\n",
    );
    let elf = scratch.path("custom.elf");
    build(&[
        "build",
        custom.to_str().unwrap(),
        "--chip",
        "rp2350",
        "-o",
        &elf,
    ]);
    check_rp2350_firmware(Path::new(&elf));
}

#[test]
fn floating_point_links_in_firmware() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("float");
    // Operands from the timer, so the optimizer cannot fold the arithmetic.
    let source = scratch.write(
        "main.dodo",
        "package main\nimport \"std/embedded/board\"\nfn main() {\n b := board.take()!\n t := b.timer.now_us()\n x := (t as f64) * 1.5 / 3.0 + 0.25 - (t as f64) % 7.0\n y := (t as f32) * 1.5f32 / 3.0f32 + 0.25f32 - (t as f32) % 7.0f32\n n := (x as i64) + (y as i64) + (x as u32) as i64 + (y as f64 as i32) as i64\n if x < y as f64 || x >= 1e300 || n == 3 || (n as f32) > 2.0f32 {\n  board.reboot_to_bootloader()\n }\n}\n",
    );
    for board in ["pico", "pico2"] {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("{board}-O{level}.elf"));
            build(&[
                "build",
                source.to_str().unwrap(),
                "--board",
                board,
                "-O",
                level,
                "-o",
                &elf,
            ]);
        }
    }
}

#[test]
fn the_default_panic_hook_reports_on_serial_ports() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("panic-report");
    // The runtime's weak panic hook calls the reporter std/embedded/chip
    // defines, which writes to the serial ports the program claimed.
    let console = root().join("examples/serial_console.dodo");
    let boards = [
        ("pico", 0xE48B_FF56),
        ("pico2", 0xE48B_FF59),
        ("pico2_w", 0xE48B_FF59),
    ];
    for (board, family) in boards {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("console-{board}-O{level}.elf"));
            let source = console.to_str().unwrap();
            build(&["build", source, "--board", board, "-O", level, "-o", &elf]);
            let report = elf_symbol(&fs::read(&elf).unwrap(), "dodo_panic_report")
                .expect("the reporter is linked");
            assert_eq!(report & 1, 1, "{board} -O{level}: a Thumb function");
            assert_eq!(
                panic_hook_literal(Path::new(&elf), family),
                report,
                "{board} -O{level}: the hook calls the reporter"
            );
        }
    }
    // Without std/embedded/chip there is no reporter: the weak reference
    // links as 0, and the hook only halts.
    let bare = scratch.write(
        "bare.dodo",
        "package main\nimport \"core/mmio\"\nfn main() {\n values: [4]u32 = [1, 2, 3, 4]\n index := unsafe { mmio.read32(0x2000_0000) } as usize\n unsafe { mmio.write32(0x2000_0004, values[index]) }\n}\n",
    );
    for (chip, family) in [("rp2040", 0xE48B_FF56), ("rp2350", 0xE48B_FF59)] {
        let elf = scratch.path(&format!("bare-{chip}.elf"));
        build(&["build", bare.to_str().unwrap(), "--chip", chip, "-o", &elf]);
        assert_eq!(
            elf_symbol(&fs::read(&elf).unwrap(), "dodo_panic_report"),
            None
        );
        assert_eq!(panic_hook_literal(Path::new(&elf), family), 0, "{chip}");
    }
}

/// The first literal of the runtime's USBCTRL_IRQ stub, which it calls if
/// nonzero: dodo_usb_interrupt's address or 0.
fn usb_interrupt_literal(elf: &Path, family: u32) -> u32 {
    stub_literal(elf, family, "USBCTRL_IRQ", 10)
}

/// The first literal after `instructions` bytes of the runtime function
/// `symbol`, in the flashed image.
fn stub_literal(elf: &Path, family: u32, symbol: &str, instructions: usize) -> u32 {
    let bytes = fs::read(elf).unwrap();
    let image = flash_image(&fs::read(elf.with_extension("uf2")).unwrap(), family);
    let stub = (elf_symbol(&bytes, symbol).expect(symbol) & !1) as usize;
    // The literal pool follows, word-aligned.
    let at = (stub - 0x1000_0000 + instructions + 3) & !3;
    word(&image, at)
}

#[test]
fn the_usb_serial_port_is_served_from_the_default_usb_interrupt() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("usb-serial");
    let example = root().join("examples/usb_serial.dodo");
    // USBCTRL_IRQ's vector: IRQ 5 on the RP2040, 14 on the RP2350, after
    // the 16 Cortex-M exceptions.
    let boards = [
        ("pico", 0xE48B_FF56, 5),
        ("pico2", 0xE48B_FF59, 14),
        ("pico2_w", 0xE48B_FF59, 14),
    ];
    for (board, family, irq) in boards {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("usb-{board}-O{level}.elf"));
            build(&[
                "build",
                example.to_str().unwrap(),
                "--board",
                board,
                "-O",
                level,
                "-o",
                &elf,
            ]);
            let bytes = fs::read(&elf).unwrap();
            let serve = elf_symbol(&bytes, "dodo_usb_interrupt").expect("the server is linked");
            assert_eq!(serve & 1, 1, "{board} -O{level}: a Thumb function");
            assert_eq!(usb_interrupt_literal(Path::new(&elf), family), serve);
            let image = flash_image(
                &fs::read(Path::new(&elf).with_extension("uf2")).unwrap(),
                family,
            );
            let vectors = elf_symbol(&bytes, "__vector_table").unwrap() as usize - 0x1000_0000;
            assert_eq!(
                word(&image, vectors + 4 * (16 + irq)),
                elf_symbol(&bytes, "USBCTRL_IRQ").unwrap() | 1,
                "{board} -O{level}: the vector is the runtime's handler"
            );
        }
    }
    // A program's own handler replaces the runtime's, and the port's
    // server is dropped with it.
    let own = scratch.write(
        "own.dodo",
        "package main\nimport \"std/embedded/board\"\nstatic mut COUNT: u32 = 0\n@interrupt(USBCTRL_IRQ)\nfn usb() {\n unsafe {\n  COUNT += 1\n }\n}\nfn main() {\n b := board.take()!\n b.led.toggle()\n}\n",
    );
    for board in ["pico", "pico2"] {
        let elf = scratch.path(&format!("own-{board}.elf"));
        build(&["build", own.to_str().unwrap(), "--board", board, "-o", &elf]);
        let bytes = fs::read(&elf).unwrap();
        assert_eq!(elf_symbol(&bytes, "dodo_usb_interrupt"), None, "{board}");
        assert!(elf_symbol(&bytes, "USBCTRL_IRQ").is_some(), "{board}");
    }
    // Without std/embedded/chip the stub falls through to DefaultHandler.
    let bare = scratch.write("bare.dodo", "package main\nfn main() {}\n");
    for (chip, family) in [("rp2040", 0xE48B_FF56), ("rp2350", 0xE48B_FF59)] {
        let elf = scratch.path(&format!("bare-{chip}.elf"));
        build(&["build", bare.to_str().unwrap(), "--chip", chip, "-o", &elf]);
        assert_eq!(usb_interrupt_literal(Path::new(&elf), family), 0, "{chip}");
    }
}

// examples/two_cores.dodo on every board: the default FIFO interrupt
// handlers serve rp_multicore's signal, and core 1 is launched at the
// runtime's trampoline.
#[test]
fn two_core_programs_link_for_every_board() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("two-cores");
    let example = root().join("examples/two_cores.dodo");
    // The FIFO interrupts: SIO_IRQ_PROC0 and SIO_IRQ_PROC1 on the RP2040,
    // SIO_IRQ_FIFO on the RP2350.
    let boards: [(&str, u32, &[usize]); 3] = [
        ("pico", 0xE48B_FF56, &[15, 16]),
        ("pico2", 0xE48B_FF59, &[25]),
        ("pico2_w", 0xE48B_FF59, &[25]),
    ];
    for (board, family, irqs) in boards {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("two-{board}-O{level}.elf"));
            build(&[
                "build",
                example.to_str().unwrap(),
                "--board",
                board,
                "-O",
                level,
                "-o",
                &elf,
            ]);
            let path = Path::new(&elf);
            let bytes = fs::read(path).unwrap();
            let serve = elf_symbol(&bytes, "dodo_fifo_interrupt").expect("the hook is linked");
            assert_eq!(serve & 1, 1, "{board} -O{level}: a Thumb function");
            // ldr, cmp, bne, ldr, bx, as in USBCTRL_IRQ.
            assert_eq!(stub_literal(path, family, "FifoHandler", 10), serve);
            let image = flash_image(&fs::read(path.with_extension("uf2")).unwrap(), family);
            let vectors = elf_symbol(&bytes, "__vector_table").unwrap() as usize - 0x1000_0000;
            let handler = elf_symbol(&bytes, "FifoHandler").unwrap() | 1;
            for irq in irqs {
                assert_eq!(
                    word(&image, vectors + 4 * (16 + irq)),
                    handler,
                    "{board} -O{level}: IRQ {irq} is the FIFO handler"
                );
            }
            let trampoline = elf_symbol(&bytes, "dodo_core1_trampoline").expect("trampoline");
            // ldr, bx, then the trampoline's address with the Thumb bit.
            assert_eq!(
                stub_literal(path, family, "dodo_core1_entry", 4),
                trampoline | 1,
                "{board} -O{level}"
            );
            assert!(
                elf_symbol(&bytes, "dodo_core_id").is_some(),
                "{board} -O{level}"
            );
        }
    }
    // Without std/embedded/chip the FIFO stub falls through to DefaultHandler.
    let bare = scratch.write("bare.dodo", "package main\nfn main() {}\n");
    for (chip, family) in [("rp2040", 0xE48B_FF56), ("rp2350", 0xE48B_FF59)] {
        let elf = scratch.path(&format!("bare-{chip}.elf"));
        build(&["build", bare.to_str().unwrap(), "--chip", chip, "-o", &elf]);
        assert_eq!(
            stub_literal(Path::new(&elf), family, "FifoHandler", 10),
            0,
            "{chip}"
        );
    }
}

// Starting core 1 checks what moves there: a task holding a reference is
// rejected, as for std/thread.
#[test]
fn core1_tasks_cannot_hold_references() {
    let scratch = Scratch::new("core1-borrow");
    let source = scratch.write(
        "borrow.dodo",
        r#"package main
import "std/embedded/board"
struct Counter {
    count: &mut u32
    pub fn run(self) {
        *self.count += 1
    }
}
fn main() {
    b := board.take()!
    count := 0u32
    stack := [0u64; 256]
    running := b.core1.start(&mut stack, Counter { count: &mut count })!
    running.stop()
}
"#,
    );
    let object = scratch.path("borrow.o");
    let error = stderr(&dodo(&[
        "build",
        source.to_str().unwrap(),
        "--board",
        "pico2",
        "--emit",
        "obj",
        "-o",
        &object,
    ]));
    assert!(
        error.contains("not checked for cross-thread transfer"),
        "{error}"
    );
}

#[test]
fn dma_signals_are_raised_from_the_default_dma_interrupt() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("dma");
    let example = root().join("examples/dma.dodo");
    // DMA_IRQ_0's vector: IRQ 11 on the RP2040, 10 on the RP2350.
    let boards = [
        ("pico", 0xE48B_FF56, 11),
        ("pico2", 0xE48B_FF59, 10),
        ("pico2_w", 0xE48B_FF59, 10),
    ];
    for (board, family, irq) in boards {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("dma-{board}-O{level}.elf"));
            build(&[
                "build",
                example.to_str().unwrap(),
                "--board",
                board,
                "-O",
                level,
                "-o",
                &elf,
            ]);
            let bytes = fs::read(&elf).unwrap();
            let serve = elf_symbol(&bytes, "dodo_dma_interrupt").expect("the handler is linked");
            assert_eq!(serve & 1, 1, "{board} -O{level}: a Thumb function");
            assert_eq!(
                stub_literal(Path::new(&elf), family, "DMA_IRQ_0", 10),
                serve
            );
            let image = flash_image(
                &fs::read(Path::new(&elf).with_extension("uf2")).unwrap(),
                family,
            );
            let vectors = elf_symbol(&bytes, "__vector_table").unwrap() as usize - 0x1000_0000;
            assert_eq!(
                word(&image, vectors + 4 * (16 + irq)),
                elf_symbol(&bytes, "DMA_IRQ_0").unwrap() | 1,
                "{board} -O{level}: the vector is the runtime's handler"
            );
        }
    }
    // A program's own handler replaces the runtime's and the default one.
    let own = scratch.write(
        "own.dodo",
        "package main\nimport \"std/embedded/board\"\nstatic mut COUNT: u32 = 0\n@interrupt(DMA_IRQ_0)\nfn dma() {\n unsafe {\n  COUNT += 1\n }\n}\nfn main() {\n b := board.take()!\n b.led.toggle()\n}\n",
    );
    for board in ["pico", "pico2"] {
        let elf = scratch.path(&format!("own-{board}.elf"));
        build(&["build", own.to_str().unwrap(), "--board", board, "-o", &elf]);
        let bytes = fs::read(&elf).unwrap();
        assert_eq!(elf_symbol(&bytes, "dodo_dma_interrupt"), None, "{board}");
        assert!(elf_symbol(&bytes, "DMA_IRQ_0").is_some(), "{board}");
    }
    // Without std/embedded/chip the stub falls through to DefaultHandler.
    let bare = scratch.write("bare.dodo", "package main\nfn main() {}\n");
    for (chip, family) in [("rp2040", 0xE48B_FF56), ("rp2350", 0xE48B_FF59)] {
        let elf = scratch.path(&format!("bare-{chip}.elf"));
        build(&["build", bare.to_str().unwrap(), "--chip", chip, "-o", &elf]);
        assert_eq!(
            stub_literal(Path::new(&elf), family, "DMA_IRQ_0", 10),
            0,
            "{chip}"
        );
    }
}

#[test]
fn usb_serial_ports_compile_for_every_board() {
    let scratch = Scratch::new("usb-compile");
    let source = scratch.write(
        "usb.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/executor"
import "std/fmt"
const USB: executor.Signal = executor.Signal { id: 3 }
fn main() {
    b := board.take()!
    serial := b.usb.serial()!
    serial.set_signal(&USB)
    serial.set_crlf(false)
    serial.set_bootloader_reset(true)
    connected := serial.is_connected()
    baud := serial.baud()
    fmt.printf(&mut serial, "{} {}\n", connected, baud)!
    buffer := [0u8; 8]
    serial.poll_read(&mut buffer)!
    serial.flush()!
    b.usb.release_serial(serial)
    analog := b.pins.analog(26)!
    again := b.usb.serial()!
    analog.read()!
    b.pins.release_analog(analog)
    b.usb.release_serial(again)
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        for level in ["0", "2"] {
            let object = scratch.path(&format!("usb-{board}-O{level}.o"));
            stdout(&dodo(&[
                "build",
                source.to_str().unwrap(),
                "--board",
                board,
                "-O",
                level,
                "--emit",
                "obj",
                "-o",
                &object,
            ]));
        }
    }
}
