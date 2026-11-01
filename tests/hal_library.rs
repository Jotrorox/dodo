//! Hardware abstraction layer: host execution with fakes, the shared
//! peripheral drivers against simulated controllers, interrupt-control
//! lowering on bare-metal targets, and rejection where no mask exists.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn dodo(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(args)
        .arg(path)
        .output()
        .unwrap()
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("dodo-hal-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn registers_protocols_and_fakes_execute_on_the_host() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("checks");
    let source = root.join("tests/stdlib/hal_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "hal-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let mut args = vec!["build", source.to_str().unwrap(), "-O", optimization];
        args.push("-o");
        success(&dodo(&args, &executable), "compile HAL fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute HAL fixture",
        );
    }
}

#[test]
fn storage_runs_against_the_fake_flash_and_survives_power_cuts() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("storage");
    let source = root.join("tests/stdlib/storage_checks.dodo");
    // Unoptimized only: the power-cut sweep runs in seconds either way, and
    // an optimized build of the fixture takes about a minute.
    let executable = scratch
        .0
        .join(format!("storage{}", std::env::consts::EXE_SUFFIX));
    let args = ["build", source.to_str().unwrap(), "-O", "0", "-o"];
    success(&dodo(&args, &executable), "compile storage fixture");
    success(
        &Command::new(&executable).output().unwrap(),
        "execute storage fixture",
    );
}

#[test]
fn polling_target_capture_can_i2s_and_dma_protocols_execute_with_fakes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("protocols");
    let source = root.join("tests/stdlib/hal_protocol_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "protocols-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile protocol fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute protocol fixture",
        );
    }
}

#[test]
fn pwm_input_capture_measures_against_a_simulated_slice() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("pwm-capture");
    let source = root.join("tests/stdlib/pwm_capture_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "pwm-capture-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile PWM capture fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute PWM capture fixture",
        );
    }
}

#[test]
fn mcp2515_can_controller_executes_against_a_simulated_chip() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("can");
    let source = root.join("tests/stdlib/can_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "can-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile CAN fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute CAN fixture",
        );
    }

    // The driver over a chip's real SPI bus and pin types, for firmware.
    for (chip, target) in [
        ("rp2040", "thumbv6m-none-eabi"),
        ("rp2350", "thumbv8m.main-none-eabi"),
    ] {
        let text = format!(
            "package can_{chip}\n\
             import \"std/embedded/can/mcp2515\"\n\
             import \"std/embedded/chip/{chip}\"\n\
             import \"std/embedded/hal\"\n\
             import \"std/embedded/peripheral/pl022\"\n\
             pub fn start(bus: pl022.Bus, select: {chip}.Pin) -> mcp2515.Controller<pl022.Bus, {chip}.Pin>!hal.Error {{\n\
             config := mcp2515.Config.new(16_000_000, 500_000)\n\
             filters: [2]hal.CanFilter = [hal.CanFilter.exact(0x123, false), hal.CanFilter.all(true)]\n\
             return mcp2515.Controller.new(bus, select, &config, &filters)\n\
             }}\n\
             pub fn echo(can: &mut mcp2515.Controller<pl022.Bus, {chip}.Pin>) -> void!hal.Error {{\n\
             frame := hal.CanFrame.new()\n\
             hal.receive_frame(can, &mut frame, hal.DEFAULT_POLL_LIMIT)?\n\
             dynamic: &mut dyn hal.Can = can\n\
             hal.send_frame(dynamic, &frame, hal.DEFAULT_POLL_LIMIT)?\n\
             return ok()\n\
             }}\n"
        );
        let file = scratch.write(&format!("{chip}.dodo"), &text);
        success(
            &dodo(
                &[
                    "build",
                    file.to_str().unwrap(),
                    "--emit",
                    "obj",
                    "--target",
                    target,
                    "-O",
                    "2",
                    "-o",
                ],
                &scratch.0.join(format!("{chip}.o")),
            ),
            &format!("compile the MCP2515 driver for {chip}"),
        );
    }
}

#[test]
fn pio_i2s_streams_execute_against_a_simulated_pio_block() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("i2s");
    let source = root.join("tests/stdlib/i2s_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "i2s-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile I2S fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute I2S fixture",
        );
    }
}

/// The instruction words of `name`, a `rp_pio.Program` constant in LLVM IR.
fn program_words(ir: &str, name: &str) -> Vec<u16> {
    let marker = format!("@dodo.main.{name} = ");
    let line = ir
        .lines()
        .find(|line| line.starts_with(&marker))
        .unwrap_or_else(|| panic!("{name} missing from the IR"));
    let start = line.find("[32 x i16] [").unwrap() + "[32 x i16] [".len();
    let end = start + line[start..].find(']').unwrap();
    let length_field = &line[end + 1..];
    let length: usize = length_field
        .trim_start_matches(", i32 ")
        .split(',')
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    line[start..end]
        .split(',')
        .take(length)
        .map(|word| {
            word.trim()
                .trim_start_matches("i16 ")
                .parse::<i16>()
                .unwrap() as u16
        })
        .collect()
}

#[test]
fn pio_i2s_programs_match_the_compiler_assembler() {
    // rp_i2s writes its programs as words so that desktop tests can run
    // them; these are what i2s_checks.dodo expects of it. The compiler's
    // own assembler must produce the same from the pioasm source.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("i2s-programs");
    let ir = scratch.0.join("programs.ll");
    success(
        &dodo(
            &[
                "build",
                root.join("tests/fixtures/rp_i2s_programs.dodo")
                    .to_str()
                    .unwrap(),
                "--board",
                "pico",
                "-O",
                "0",
                "--emit",
                "llvm-ir",
                "-o",
            ],
            &ir,
        ),
        "assemble the I2S programs",
    );
    let text = fs::read_to_string(&ir).unwrap();
    let expected: [(&str, &[u16]); 8] = [
        (
            "OUT32_PHILIPS",
            &[
                0x6101, 0xE95D, 0x6101, 0x0982, 0x7001, 0x1027, 0x1908, 0x99A0, 0x7101, 0xF85D,
                0xB825, 0x7101, 0x198B, 0x6001, 0x0030, 0x0900, 0x89A0,
            ],
        ),
        (
            "OUT32_LEFT",
            &[
                0x7101, 0xF95D, 0x7101, 0x1982, 0x7001, 0x1027, 0x1908, 0x99A0, 0x6101, 0xE85D,
                0xA825, 0x6101, 0x098B, 0x6001, 0x0030, 0x0900, 0x89A0,
            ],
        ),
        (
            "OUT16_PHILIPS",
            &[
                0x6101, 0xE94D, 0x6101, 0x0982, 0x7101, 0xF94E, 0x7101, 0x1986, 0x6101, 0x8980,
            ],
        ),
        (
            "OUT16_LEFT",
            &[
                0x7101, 0xF94D, 0x7101, 0x1982, 0x7101, 0xF94E, 0x6101, 0x0986, 0x6101, 0x8980,
            ],
        ),
        (
            "IN32_LEFT",
            &[
                0xF15E, 0x5901, 0x1181, 0x5901, 0xE15E, 0x4901, 0x0185, 0x4901,
            ],
        ),
        (
            "IN32_PHILIPS",
            &[
                0xE15E, 0x4901, 0x0181, 0x4901, 0xF15E, 0x5901, 0x1185, 0x5901,
            ],
        ),
        (
            "IN16_PHILIPS",
            &[
                0xE14E, 0x4901, 0x0181, 0x4901, 0xF14E, 0x5901, 0x1185, 0x5901,
            ],
        ),
        ("EXECS", &[0xA0E3, 0xA02B, 0xE020, 0xE05E]),
    ];
    for (name, words) in expected {
        assert_eq!(program_words(&text, name), words, "{name}");
    }
}

#[test]
fn executor_runs_tasks_against_the_simulated_clock() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("executor");
    let source = root.join("tests/stdlib/executor_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "executor-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile executor fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute executor fixture",
        );
    }
}

#[test]
fn power_protocol_and_rp_clock_gating_execute_on_the_host() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("power");
    let source = root.join("tests/stdlib/power_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "power-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile power fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute power fixture",
        );
    }
}

#[test]
fn core1_launch_and_shared_parts_execute_against_a_simulated_boot_rom() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("multicore");
    let source = root.join("tests/stdlib/multicore_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "multicore-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile multicore fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute multicore fixture",
        );
    }
}

#[test]
fn cyw43_driver_executes_against_simulated_pins_and_chip() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("cyw43");
    let source = root.join("tests/stdlib/cyw43_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "cyw43-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile CYW43 fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute CYW43 fixture",
        );
    }
}

#[test]
fn peripheral_drivers_execute_against_simulated_controllers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("peripheral");
    let source = root.join("tests/stdlib/peripheral_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "peripheral-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile peripheral fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute peripheral fixture",
        );
    }
}

#[test]
fn espressif_drivers_execute_against_simulated_controllers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("esp-peripheral");
    let source = root.join("tests/stdlib/esp_peripheral_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "esp-peripheral-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(
            &dodo(&args, &executable),
            "compile Espressif peripheral fixture",
        );
        success(
            &Command::new(&executable).output().unwrap(),
            "execute Espressif peripheral fixture",
        );
    }
}

#[test]
fn real_time_clocks_execute_against_fakes_and_simulated_registers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("rtc");
    let source = root.join("tests/stdlib/rtc_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "rtc-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile rtc fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute rtc fixture",
        );
    }
}

#[test]
fn watchdog_executes_against_a_simulated_watchdog() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("watchdog");
    let source = root.join("tests/stdlib/watchdog_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "watchdog-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile watchdog fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute watchdog fixture",
        );
    }
}

#[test]
fn dma_channels_and_sessions_execute_against_simulated_controllers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("dma");
    let source = root.join("tests/stdlib/dma_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "dma-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile dma fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute dma fixture",
        );
    }
}

#[test]
fn usb_serial_device_enumerates_and_moves_data_against_a_simulated_host() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("usb");
    let source = root.join("tests/stdlib/usb_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "usb-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile usb fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute usb fixture",
        );
    }
}

#[test]
fn sensor_example_tests_pass_against_fakes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = dodo(&["test"], &root.join("examples/hal_sensor.dodo"));
    success(&output, "test examples/hal_sensor.dodo");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("3 passed"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn dimmer_example_tests_pass_against_fakes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = dodo(&["test"], &root.join("examples/dimmer.dodo"));
    success(&output, "test examples/dimmer.dodo");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("2 passed"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn sensor_log_example_propagates_bus_and_stream_errors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = dodo(&["test"], &root.join("examples/sensor_log.dodo"));
    success(&output, "test examples/sensor_log.dodo");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("3 passed"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn interrupt_control_lowers_to_each_architecture_mask() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("lowering");
    let source = root.join("tests/stdlib/hal_checks.dodo");
    for (target, expected) in [
        ("thumbv6m-none-eabi", &["mrs", "cpsid", "msr"][..]),
        ("thumbv7em-none-eabihf", &["mrs", "cpsid", "msr"][..]),
        ("riscv32-unknown-none-elf", &["csrrci", "csrs"][..]),
        ("riscv64-unknown-none-elf", &["csrrci", "csrs"][..]),
    ] {
        let assembly = scratch.0.join(format!("{target}.s"));
        success(
            &dodo(
                &[
                    "build",
                    source.to_str().unwrap(),
                    "--emit",
                    "asm",
                    "--target",
                    target,
                    "-O",
                    "3",
                    "-o",
                ],
                &assembly,
            ),
            &format!("emit HAL assembly for {target}"),
        );
        let text = fs::read_to_string(&assembly).unwrap().to_lowercase();
        for instruction in expected {
            assert!(
                text.contains(instruction),
                "{target} lacks {instruction}:\n{text}"
            );
        }
    }

    let cpu = scratch.write(
        "cpu.dodo",
        "package cpu_ops\nimport \"core/cpu\"\npub fn idle() {\n cpu.fence()\n cpu.wait_for_interrupt()\n}\n",
    );
    let assembly = scratch.0.join("idle.s");
    success(
        &dodo(
            &[
                "build",
                cpu.to_str().unwrap(),
                "--emit",
                "asm",
                "--target",
                "thumbv6m-none-eabi",
                "-o",
            ],
            &assembly,
        ),
        "emit wait-for-interrupt",
    );
    let text = fs::read_to_string(&assembly).unwrap();
    assert!(text.contains("wfi") && text.contains("dmb"), "{text}");
    // RISC-V waits through the chip's weakly referenced
    // dodo_wait_for_interrupt, which the ESP32-C6 package defines for its
    // deep sleep, and fall back to a plain wfi without one.
    let assembly = scratch.0.join("idle-riscv.s");
    success(
        &dodo(
            &[
                "build",
                cpu.to_str().unwrap(),
                "--emit",
                "asm",
                "--target",
                "riscv32-unknown-none-elf",
                "-o",
            ],
            &assembly,
        ),
        "emit RISC-V wait-for-interrupt",
    );
    let text = fs::read_to_string(&assembly).unwrap();
    assert!(
        text.contains("wfi")
            && text.contains("dodo_wait_for_interrupt")
            && text.contains(".weak"),
        "{text}"
    );

    // Deep sleep and sleep-on-exit change the SCB's SCR at 0xE000ED10.
    let scr = scratch.write(
        "scr.dodo",
        "package scr\nimport \"std/embedded/cortex_m\"\npub fn deeper() -> bool {\n cortex_m.set_deep_sleep(true)\n cortex_m.set_sleep_on_exit(false)\n return cortex_m.deep_sleep()\n}\n",
    );
    let assembly = scratch.0.join("scr.s");
    success(
        &dodo(
            &[
                "build",
                scr.to_str().unwrap(),
                "--emit",
                "asm",
                "--target",
                "thumbv6m-none-eabi",
                "-O",
                "2",
                "-o",
            ],
            &assembly,
        ),
        "emit SCR access",
    );
    let text = fs::read_to_string(&assembly).unwrap().to_lowercase();
    assert!(
        text.contains("3758157072") || text.contains("0xe000ed10"),
        "{text}"
    );

    // The core number: Cortex-M asks the chip runtime's dodo_core_id, if
    // there is one, and RISC-V reads mhartid.
    let core_id = scratch.write(
        "core_id.dodo",
        "package core_number\nimport \"core/cpu\"\npub fn here() -> u32 {\n return cpu.core_id()\n}\n",
    );
    for (target, expected) in [
        ("thumbv6m-none-eabi", &[".weak", "dodo_core_id"][..]),
        ("thumbv8m.main-none-eabi", &[".weak", "dodo_core_id"][..]),
        ("riscv32-unknown-none-elf", &["mhartid"][..]),
    ] {
        let assembly = scratch.0.join(format!("core_id-{target}.s"));
        success(
            &dodo(
                &[
                    "build",
                    core_id.to_str().unwrap(),
                    "--emit",
                    "asm",
                    "--target",
                    target,
                    "-o",
                ],
                &assembly,
            ),
            &format!("emit core_id for {target}"),
        );
        let text = fs::read_to_string(&assembly).unwrap();
        for needle in expected {
            assert!(text.contains(needle), "{target} lacks {needle}:\n{text}");
        }
    }
}

#[test]
fn freestanding_targets_without_an_interrupt_model_are_rejected() {
    let scratch = Scratch::new("rejected");
    let critical = scratch.write(
        "critical.dodo",
        "package critical\nimport \"std/embedded/hal\"\npub fn section() {\n guard := hal.Critical.enter()\n core.drop(guard)\n}\n",
    );
    let output = dodo(
        &[
            "build",
            critical.to_str().unwrap(),
            "--emit",
            "obj",
            "--target",
            "wasm32-unknown-unknown",
            "-o",
        ],
        &scratch.0.join("critical.o"),
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not supported for target"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The rest of the HAL stays portable to the same target.
    let registers = scratch.write(
        "registers.dodo",
        "package registers\nimport \"std/embedded/hal\"\npub fn enable(address: usize) {\n r: hal.Reg<u32> = unsafe { hal.Reg.at(address) }\n r.set(0x70, 3)\n}\n",
    );
    success(
        &dodo(
            &[
                "build",
                registers.to_str().unwrap(),
                "--emit",
                "obj",
                "--target",
                "wasm32-unknown-unknown",
                "-o",
            ],
            &scratch.0.join("registers.o"),
        ),
        "portable registers on wasm",
    );
}

#[test]
fn field_values_that_do_not_fit_trap() {
    let scratch = Scratch::new("trap");
    let source = scratch.write(
        "main.dodo",
        "package main\nimport \"std/embedded/hal\"\nfn main() {\n value := hal.encode(0x70u32, 8)\n core.assert_eq(value, 0u32)\n}\n",
    );
    let executable = scratch
        .0
        .join(format!("trap{}", std::env::consts::EXE_SUFFIX));
    success(
        &dodo(&["build", source.to_str().unwrap(), "-o"], &executable),
        "compile trapping fixture",
    );
    let output = Command::new(&executable).output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("assert"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn espressif_low_power_executes_against_simulated_registers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("esp-power");
    let source = root.join("tests/stdlib/esp_power_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "esp-power-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let args = ["build", source.to_str().unwrap(), "-O", optimization, "-o"];
        success(&dodo(&args, &executable), "compile Espressif power fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute Espressif power fixture",
        );
    }
}
