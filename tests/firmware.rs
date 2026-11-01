//! Firmware platforms: board and chip settings, `std/embedded/board` and `std/embedded/chip`
//! resolution, the embedded chip runtimes, and (when ld.lld is installed)
//! linking flashable images. Running on a real board is scripts/test_pico.py
//! (`--board pico`, `--board pico2`, or `--board pico2_w`) and
//! scripts/test_arduino_uno.py; running on emulated chips is
//! scripts/test_emulator.py.
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

// Every board's timer is a hal counter and delay, and its alarms are hal
// alarms: drivers generic over the protocols take them unchanged.
#[test]
fn timer_protocols_compile_for_every_board() {
    let scratch = Scratch::new("timer-protocols");
    let source = scratch.write(
        "timers.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/hal"
fn settle<T: hal.Delay + hal.Counter>(timer: &mut T) -> u64 {
    start := timer.now_us()
    deadline := hal.Deadline.after(timer, 2000)
    for !deadline.passed(timer) {
        timer.delay_us(100)
    }
    return hal.elapsed_us(timer, start)
}
fn pulse<A: hal.Alarm>(alarm: &mut A) -> void!hal.Error {
    hal.schedule_in_us(alarm, 500)?
    hal.wait_for_alarm(alarm)
    alarm.schedule_at_us(0)?
    core.assert(alarm.fired())
    alarm.clear()
    alarm.cancel()
    return ok()
}
fn main() {
    b := board.take()!
    core.assert(settle(&mut b.timer) >= 2000)
    ticker := hal.Ticker.start(&b.timer, 1000)
    ticker.wait(&b.timer)
    alarm := b.timer.alarm(1)!
    pulse(&mut alarm)!
    far := alarm.now_us() + 0x8000_0000
    match alarm.schedule_at_us(far) {
        ok() => core.assert(false),
        err(error) => core.assert(error == hal.Error.InvalidInput),
    }
    b.timer.release_alarm(alarm)
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        for level in ["0", "2"] {
            let object = scratch.path(&format!("timers-{board}-O{level}.o"));
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

// The timer protocols at other rates on a board: a timer alarm as a
// compare channel and as a periodic timer, a PWM slice as a periodic timer
// and tick counter, a PWM output's wrap interrupt, a PIO edge capture, and
// on the RP2350 TIMER1, through generic drivers, with interrupt handlers
// for the alarm and the PWM wrap in the vector table.
#[test]
fn timer_channels_link_for_every_board() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("timer-channels");
    let common = r#"package main
import "std/embedded/board"
import "std/embedded/chip"
import "std/embedded/chip/rp_capture"
import "std/embedded/chip/rp_pwm"
import "std/embedded/cortex_m"
import "std/embedded/hal"
static mut ALARM: Option<chip.Alarm> = none
static mut TIMER: Option<rp_pwm.PwmTimer> = none
static mut PERIODS: u32 = 0
@interrupt(ALARM_VECTOR)
fn alarm_fired() {
    unsafe {
        match &mut ALARM {
            some(alarm) => {
                PERIODS += alarm.poll_periods()
            }
            none => {}
        }
    }
}
@interrupt(PWM_VECTOR)
fn wrapped() {
    unsafe {
        match &mut TIMER {
            some(timer) => {
                PERIODS += timer.poll_periods()
            }
            none => {}
        }
    }
}
fn measure<C: hal.TickCounter>(counter: &C) -> u64 {
    start := counter.now_ticks()
    for hal.elapsed_ticks(counter, start) < 100 {}
    return hal.ticks_to_ns(hal.elapsed_ticks(counter, start), counter.tick_hz())
}
fn pulse<C: hal.Compare>(channel: &mut C) -> void!hal.Error {
    hal.schedule_in_ticks(channel, 1000)?
    hal.wait_for_compare(channel)
    return ok()
}
fn every<P: hal.PeriodicTimer>(timer: &mut P, period_us: u64) -> u32!hal.Error {
    hal.start_periodic_us(timer, period_us)?
    timer.listen()
    return ok(hal.wait_periods(timer))
}
fn edges<C: hal.InputCapture>(input: &mut C) -> u64!hal.Error {
    return hal.capture_ticks(input, 1_000_000)
}
fn main() {
    b := board.take()!
    alarm := b.timer.alarm(1)!
    pulse(&mut alarm)!
    core.assert(every(&mut alarm, 500)! >= 1)
    core.assert(measure(&b.timer) >= 100_000)
    irq := alarm.irq()
    unsafe {
        ALARM = some(alarm)
    }
    cortex_m.enable(irq)
    timer := b.pins.pwm_timer(3, 1_000_000)!
    core.assert(every(&mut timer, 100)! >= 1)
    core.assert(measure(&timer) >= 100_000)
    wrap := timer.irq()
    unsafe {
        TIMER = some(timer)
    }
    cortex_m.enable(wrap)
    output := b.pins.pwm(0, 1000)!
    output.listen_wrap()
    for !output.wrapped() {}
    output.clear_wrap()
    output.ignore_wrap()
    b.pins.release_pwm(output)
    pio := b.pins.pio(0)!
    capture := b.pins.edge_capture(&mut pio, 5, rp_capture.RISING, 1_000_000)!
    match edges(&mut capture) {
        ok(ticks) => core.assert(ticks > 0),
        err(error) => core.assert(error == hal.Error.Timeout),
    }
    b.pins.release_edge_capture(capture, &mut pio)
    b.pins.release_pio(pio)
    extra(&mut b)
}
"#;
    let rp2040 = "fn extra(b: &mut board.Board) {}\n";
    let rp2350 = r#"fn extra(b: &mut board.Board) {
    second := b.timer.timer1(chip.SYS_HZ)!
    core.assert(measure(&second) >= 100_000)
    channel := second.alarm(0)!
    pulse(&mut channel)!
    core.assert(every(&mut channel, 1000)! >= 1)
    hal.schedule_in_us(&mut channel, 10)!
    second.release_alarm(channel)
    b.timer.release_timer1(second)
    slow := b.timer.timer1(100_000)!
    core.assert(slow.tick_hz() == 100_000)
    b.timer.release_timer1(slow)
}
"#;
    // The alarm's and the PWM wrap's vectors and interrupt numbers.
    let boards = [
        (
            "pico",
            0xE48B_FF56u32,
            "TIMER_IRQ_1",
            1usize,
            "PWM_IRQ_WRAP",
            4usize,
            rp2040,
        ),
        (
            "pico2",
            0xE48B_FF59,
            "TIMER0_IRQ_1",
            1,
            "PWM_IRQ_WRAP_0",
            8,
            rp2350,
        ),
        (
            "pico2_w",
            0xE48B_FF59,
            "TIMER0_IRQ_1",
            1,
            "PWM_IRQ_WRAP_0",
            8,
            rp2350,
        ),
    ];
    for (board, family, alarm, alarm_irq, wrap, wrap_irq, extra) in boards {
        let text = common
            .replace("ALARM_VECTOR", alarm)
            .replace("PWM_VECTOR", wrap)
            + extra;
        let source = scratch.write(&format!("timers-{board}.dodo"), &text);
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("timers-{board}-O{level}.elf"));
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
            let path = Path::new(&elf);
            let bytes = fs::read(path).unwrap();
            let image = flash_image(&fs::read(path.with_extension("uf2")).unwrap(), family);
            let vectors = elf_symbol(&bytes, "__vector_table").unwrap() as usize - 0x1000_0000;
            for (name, irq) in [(alarm, alarm_irq), (wrap, wrap_irq)] {
                assert_eq!(
                    word(&image, vectors + 4 * (16 + irq)),
                    elf_symbol(&bytes, name).unwrap() | 1,
                    "{board} -O{level}: {name}"
                );
            }
        }
    }
}

// Low power on a board: the chip's Power through the portable protocol
// and directly, and the example. Needs no linker.
#[test]
fn power_controls_compile_for_every_board() {
    let scratch = Scratch::new("power");
    let example = root().join("examples/low_power.dodo");
    let source = scratch.write(
        "power.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip"
import "std/embedded/hal"
import "std/embedded/power"
fn idle<P: power.Power>(chip_power: &mut P, keep: power.Clocks) -> power.Clocks {
    stopped := chip_power.disable_unused_clocks()
    chip_power.set_sleep_clocks(keep)
    chip_power.set_mode(power.Mode.DeepSleep)
    chip_power.sleep()
    chip_power.set_mode(power.Mode.Sleep)
    return stopped
}
fn main() {
    b := board.take()!
    stopped := idle(&mut b.power, chip.ALARM_CLOCKS | chip.CLOCK_GPIO)
    core.assert((stopped & chip.ESSENTIAL_CLOCKS) == 0)
    match b.power.disable_clocks(chip.ESSENTIAL_CLOCKS) {
        ok() => core.assert(false),
        err(error) => core.assert(error == hal.Error.InvalidInput),
    }
    b.power.disable_clocks(chip.CLOCK_UART1)!
    serial := b.pins.uart(4, none, 9600)!
    core.assert((b.power.enabled_clocks() & chip.CLOCK_UART1) == chip.CLOCK_UART1)
    b.pins.release_uart(serial)
    power.deep_sleep(&mut b.power, chip.ALARM_CLOCKS)
    button := b.pins.input(15)!
    wake := power.Wake.new()
    wake.on(15, power.RISING_EDGE | power.LEVEL_HIGH)
    woken := b.power.dormant(&wake)!
    core.assert(woken <= 1u64 << 15)
    b.pins.release(button)
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        for (input, name) in [(source.as_path(), "power"), (example.as_path(), "example")] {
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

// Waking a dormant chip by time, a slower system clock, and the real-time
// clocks' other tick sources on every board, and the RP2350's power-down
// states, linked into firmware.
#[test]
fn low_power_states_link_for_every_board() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("low-power");
    let portable = scratch.write(
        "timed.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip"
import "std/embedded/hal"
import "std/embedded/power"
fn nap<P: power.Power>(chip_power: &mut P, milliseconds: u64) -> bool!hal.Error {
    woken := power.dormant_for(chip_power, milliseconds)?
    return ok(power.by_time(woken))
}
fn main() {
    b := board.take()!
    // A 32.768 kHz oscillator on GPIO 20 keeps the clock running.
    b.rtc.use_clock_input(&mut b.pins, 20, 32_768)!
    b.rtc.set_ms(1_800_000_000_000)!
    crystal := b.power.crystal_hz()
    b.power.set_system_hz(crystal)!
    core.assert(b.power.system_hz() == crystal)
    button := b.pins.input(15)!
    wake := power.Wake.new()
    wake.on(15, power.FALLING_EDGE)
    wake.after_ms(60_000)
    woken := b.power.dormant(&wake)!
    core.assert(power.by_time(woken) || power.woken_pins(woken) <= 1u64 << 15)
    core.assert(nap(&mut b.power, 1000)!)
    b.power.set_system_hz(chip.SYS_HZ)!
    serial := b.pins.uart(4, none, 9600)!
    match b.power.set_system_hz(crystal / 2) {
        ok() => core.assert(false),
        err(error) => core.assert(error == hal.Error.Busy),
    }
    b.pins.release_uart(serial)
    b.rtc.release_clock_input(&mut b.pins)!
    b.pins.release(button)
}
"#,
    );
    let power_down = scratch.write(
        "power_down.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip"
import "std/embedded/chip/rp_rtc"
import "std/embedded/hal"
import "std/embedded/power"
// Survives the power-down with SRAM0 kept.
@section(".noinit")
static mut WAKES: u32 = 0
fn main() {
    b := board.take()!
    if b.power.woke_from_power_down() && (b.power.retained() & chip.RETAIN_SRAM0) != 0 {
        unsafe {
            WAKES += 1
        }
        core.assert(b.power.power_down_wake() != 0)
    } else {
        unsafe {
            WAKES = 0
        }
    }
    count := b.power.scratch(0)
    b.power.set_scratch(chip.SCRATCH_WORDS - 1, count + 1)
    b.rtc.use_low_power_oscillator()!
    core.assert(b.rtc.tick_source() == rp_rtc.TICK_LPOSC)
    b.rtc.use_crystal()!
    if !b.rtc.is_set()! {
        b.rtc.set_ms(1_800_000_000_000)!
    }
    button := b.pins.input(15)!
    wake := power.Wake.new()
    wake.on(15, power.FALLING_EDGE)
    wake.after_ms(3_600_000)
    match b.power.power_down(&wake, chip.RETAIN_SRAM0) {
        ok() => {}
        err(error) => core.assert(error != hal.Error.InvalidInput),
    }
    b.pins.release(button)
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        let mut sources = vec![(portable.as_path(), "timed")];
        if board != "pico" {
            sources.push((power_down.as_path(), "power-down"));
        }
        for (input, name) in sources {
            for level in ["0", "2"] {
                let elf = scratch.path(&format!("{name}-{board}-O{level}.elf"));
                build(&[
                    "build",
                    input.to_str().unwrap(),
                    "--board",
                    board,
                    "-O",
                    level,
                    "-o",
                    &elf,
                ]);
                if board == "pico" {
                    check_rp2040_firmware(Path::new(&elf));
                } else {
                    check_rp2350_firmware(Path::new(&elf));
                }
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
// GPIOs up to the package's last, PIO windows, and pin interrupts on
// either core: a core 1 task listens to a pin, and the one IO_IRQ_BANK0
// handler serves it there. The Pico boards' chips have 30 GPIOs, so the
// RP2350B's GPIO 30 to 47 are reached only through pin_count().
#[test]
fn gpio_banks_and_core1_pin_interrupts_link_for_every_board() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("gpio-banks");
    let source = scratch.write(
        "gpio.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip"
import "std/embedded/chip/rp_pio"
import "std/embedded/cortex_m"
import "std/embedded/executor"
import "std/embedded/hal"
import "std/embedded/power"
const EDGE: executor.Signal = executor.Signal { id: 0 }
const WAITS: rp_pio.Program = rp_pio.assemble(".side_set 1", "    wait 1 gpio 20 side 0", "    in pins, 1     side 1", "    push           side 0")
@interrupt(IO_IRQ_BANK0)
fn gpio_interrupt() {
    // Runs on the core that listens: core 1 here.
    cortex_m.disable(chip.IO_IRQ_BANK0)
    EDGE.raise()
}
struct Watcher {
    button: chip.Pin
    pub fn run(self) {
        self.button.listen(chip.FALLING_EDGE | chip.RISING_EDGE)
        cortex_m.enable(self.button.irq())
        for {
            if self.button.events_on(1) != 0 {
                self.button.clear_events(chip.FALLING_EDGE | chip.RISING_EDGE)
                cortex_m.enable(chip.IO_IRQ_BANK0)
            }
        }
    }
}
fn main() {
    b := board.take()!
    count := b.pins.pin_count()
    core.assert(count == chip.pin_count() && (count == chip.PIN_COUNT || count == 48))
    last := b.pins.input(count - 1)!
    core.assert(last.number() == count - 1)
    last.listen_on(0, chip.LEVEL_HIGH)
    core.assert(last.events_on(0) <= chip.LEVEL_HIGH)
    last.ignore_on(0, chip.LEVEL_HIGH)
    b.pins.release(last)
    match b.pins.output(count) {
        ok(_) => core.assert(false),
        err(error) => core.assert(error == hal.Error.InvalidInput),
    }
    pio := b.pins.pio(1)!
    match pio.set_gpio_base(rp_pio.HIGH_GPIO_BASE) {
        ok() => {}
        err(error) => core.assert(error == hal.Error.Unsupported),
    }
    base := pio.gpio_base()
    b.pins.pio_pins(&pio, base + 4, 2)!
    loaded := pio.load(&WAITS)!
    config := loaded.config()
    config.set_in_pins(base + 4)!
    config.set_sideset_pins(base + 5)!
    config.set_jmp_pin(base + 31)!
    sm := pio.free_state_machine()!
    sm.init(&config)
    sm.set_pindirs(base + 5, 1, 1)
    pio.bypass_input_sync(base + 4, true)
    sm.set_enabled(true)
    pio.release_state_machine(sm)
    pio.unload(loaded)
    b.pins.release_pio(pio)
    wake := power.Wake.new()
    wake.on(count - 1, power.LEVEL_HIGH)
    woken := b.power.dormant(&wake)!
    core.assert(woken <= 1u64 << (count - 1) as u64)
    button := b.pins.input(15)!
    stack := [0u64; 256]
    watching := b.core1.start(&mut stack, Watcher { button })!
    b.timer.delay_ms(10)
    watching.stop()
}
"#,
    );
    // IO_IRQ_BANK0 is IRQ 13 on the RP2040 and 21 on the RP2350; both
    // cores take it through the one vector table.
    let boards: [(&str, u32, usize); 3] = [
        ("pico", 0xE48B_FF56, 13),
        ("pico2", 0xE48B_FF59, 21),
        ("pico2_w", 0xE48B_FF59, 21),
    ];
    for (board, family, irq) in boards {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("gpio-{board}-O{level}.elf"));
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
            let path = Path::new(&elf);
            let bytes = fs::read(path).unwrap();
            let handler = elf_symbol(&bytes, "IO_IRQ_BANK0").expect("the handler is linked");
            let image = flash_image(&fs::read(path.with_extension("uf2")).unwrap(), family);
            let vectors = elf_symbol(&bytes, "__vector_table").unwrap() as usize - 0x1000_0000;
            assert_eq!(
                word(&image, vectors + 4 * (16 + irq)),
                handler | 1,
                "{board} -O{level}: IRQ {irq} is the program's IO_IRQ_BANK0 handler"
            );
        }
    }
}

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
    // The Raspberry Pi chips' runtimes; AVR's has its own test below.
    for chip in dodoc::hardware::CHIPS
        .iter()
        .filter(|chip| chip.name.starts_with("rp"))
    {
        let object = scratch.0.join(format!("{}.o", chip.name));
        let options = dodoc::codegen::Options {
            target: Some(chip.triple.into()),
            cpu: Some(chip.cpu.into()),
            features: chip.features.into(),
            optimization: 2,
            ..Default::default()
        };
        dodoc::codegen::compile_ir_object(&options, chip.name, &(chip.runtime)(), &object).unwrap();
        let bytes = fs::read(&object).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for name in [
            "Reset_Handler",
            chip.panic_hook,
            "dodo_panic_report",
            "dodo_usb_interrupt",
            "dodo_dma_interrupt",
            "dodo_flash_erase",
            "dodo_flash_program",
            "dodo_flash_image_end",
            "dodo_flash_end",
            "memcpy",
            "memset",
            "__udivdi3",
        ] {
            assert!(text.contains(name), "{} runtime lacks {name}", chip.name);
        }
        let own: &[&str] = if chip.name.starts_with("rp") {
            &[
                "USBCTRL_IRQ",
                "DMA_IRQ_0",
                "FifoHandler",
                "dodo_fifo_interrupt",
                "dodo_core_id",
                "dodo_core1_trampoline",
                "dodo_core1_entry",
            ]
        } else {
            &[
                ".boot_header",
                "__vector_table",
                "dodo_trap_interrupt",
                "dodo_trap_exception",
                "dodo_interrupt_table",
                "dodo_esp32c6_flash_setup",
                "Exception_Handler",
                "USB_DEVICE",
                "DMA_IN_CH0",
            ]
        };
        for name in own {
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
            // Regions erase and write under rp_multicore's lockout, whose
            // handler parks the other core in SRAM.
            assert!(in_sram(symbol("dodo_lockout_handler")), "{board}");
            assert!(in_sram(symbol("dodo_lockout_mailbox")), "{board}");
            assert!(symbol("dodo.main.rp_multicore.lockout") >= 0x1000_0000);
        }
    }
}

/// The lockout handler's code as loaded: the bytes `length` from the start
/// of `dodo_lockout_handler`, taken from the flash image of `.data`.
fn lockout_handler_code(elf: &Path, family: u32, length: usize) -> Vec<u8> {
    let bytes = fs::read(elf).unwrap();
    let image = flash_image(&fs::read(elf.with_extension("uf2")).unwrap(), family);
    let symbol = |name: &str| elf_symbol(&bytes, name).unwrap_or_else(|| panic!("{name}"));
    let handler = symbol("dodo_lockout_handler") & !1;
    let at = (symbol("__data_load") + handler - symbol("__data_start") - 0x1000_0000) as usize;
    image[at..at + length].to_vec()
}

// A flash region erased and written on each core while the other runs a
// task from flash, and rp_multicore.lockout called directly, on every board:
// the lockout handler is IO_IRQ_QSPI's vector and runs from SRAM, with its
// literal pool, which holds IO_QSPI's PROC0_INTF clear alias of the chip.
#[test]
fn flash_writes_with_both_cores_running_link_for_every_board() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("lockout");
    let source = scratch.write(
        "lockout.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip/rp_flash"
import "std/embedded/chip/rp_multicore"
import "std/embedded/hal"
import "std/embedded/storage"

static mut STACK: [1024]u64 = [0u64; 1024]

struct Writer {
    region: rp_flash.Region

    pub fn run(self) {
        let Writer { region } = self
        match storage.Store.open(region) {
            ok(store) => {
                settings := store
                match settings.set_u32("core", 1) {
                    ok() => {}
                    err(_) => {}
                }
            }
            err(_) => {}
        }
        match rp_multicore.lockout() {
            ok(pause) => {
                core.assert(pause.is_parked())
            }
            err(error) => {
                core.assert(error == hal.Error.Timeout || error == hal.Error.Busy)
            }
        }
    }
}

fn main() {
    b := board.take()!
    capacity := b.flash.capacity()
    theirs := b.flash.region(capacity - 32 * 1024, 16 * 1024)!
    mine := b.flash.last(16 * 1024)!
    running := unsafe {
        b.core1.start(&mut STACK, Writer { region: theirs })!
    }
    store := storage.Store.open(mine)!
    store.set_u32("core", 0)!
    pause := rp_multicore.lockout()!
    inner := rp_multicore.lockout()!
    core.assert(pause.is_parked() && !inner.is_parked())
    core.drop(inner)
    core.drop(pause)
    running.stop()
}
"#,
    );
    // IO_IRQ_QSPI, and IO_QSPI's PROC0_INTF clear alias.
    let boards = [
        ("pico", 0xE48B_FF56u32, 14usize, 0x4001_B038u32),
        ("pico2", 0xE48B_FF59, 23, 0x4003_3220),
        ("pico2_w", 0xE48B_FF59, 23, 0x4003_3220),
    ];
    for (board, family, irq, force) in boards {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("lockout-{board}-O{level}.elf"));
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
            let path = Path::new(&elf);
            let bytes = fs::read(path).unwrap();
            let symbol = |name: &str| {
                elf_symbol(&bytes, name).unwrap_or_else(|| panic!("{board} lacks {name}"))
            };
            let handler = symbol("dodo_lockout_handler");
            let in_sram = |address: u32| (0x2000_0000..0x2008_2000).contains(&address);
            assert!(
                in_sram(handler),
                "{board} -O{level}: the handler runs from SRAM"
            );
            assert!(in_sram(symbol("dodo_lockout_mailbox")));
            let image = flash_image(&fs::read(path.with_extension("uf2")).unwrap(), family);
            let vectors = symbol("__vector_table") as usize - 0x1000_0000;
            assert_eq!(
                word(&image, vectors + 4 * (16 + irq)),
                handler | 1,
                "{board} -O{level}: IO_IRQ_QSPI is the lockout handler"
            );
            // Its literals follow its code in SRAM, so it reads nothing from
            // flash: among them the forced bit's clear alias and the
            // mailbox.
            let code = lockout_handler_code(path, family, 0x48);
            let literals: Vec<u32> = (0..code.len() / 4).map(|at| word(&code, 4 * at)).collect();
            assert!(
                literals.contains(&force),
                "{board} -O{level}: {literals:x?}"
            );
            assert!(literals.contains(&symbol("dodo_lockout_mailbox")));
            assert!(symbol("dodo.main.rp_multicore.lockout") >= 0x1000_0000);
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
            // Core 1 and core 0 listen for the lockout through IO_IRQ_QSPI,
            // whose handler runs from SRAM.
            let qspi = if board == "pico" { 14 } else { 23 };
            let lockout = elf_symbol(&bytes, "dodo_lockout_handler").expect("lockout");
            assert!((0x2000_0000..0x2008_2000).contains(&lockout), "{board}");
            assert_eq!(word(&image, vectors + 4 * (16 + qspi)), lockout | 1);
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
fn adc_samplers_compile_for_every_board() {
    let scratch = Scratch::new("adc-sampler");
    let source = scratch.write(
        "sampler.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip/rp_adc"
import "std/embedded/executor"
import "std/embedded/hal"
const READY: executor.Signal = executor.Signal { id: 0 }
fn main() {
    b := board.take()!
    sampler := b.pins.sampler(rp_adc.GPIO26 | rp_adc.GPIO27 | rp_adc.TEMPERATURE, 30_000)!
    match b.pins.analog(28) {
        ok(input) => core.assert(false),
        err(error) => core.assert(error == hal.Error.Busy),
    }
    sampler.set_threshold(4)!
    sampler.set_signal(&READY)
    sampler.start()
    samples: [16]u16 = [0; 16]
    got := sampler.poll_read(&mut samples)!
    sampler.read(&mut samples)!
    value := rp_adc.sample_value(samples[0])
    input := rp_adc.sample_input(samples[0])
    failed := rp_adc.sample_failed(samples[0])
    sampler.stop()
    sampler.clear_signal()
    channel := b.dma.channel()!
    capture: [256]u16 = [0; 256]
    sampler.capture(&mut channel, &mut capture)!
    bytes: [256]u8 = [0; 256]
    sampler.capture_bytes(&mut channel, &mut bytes)!
    {
        session := sampler.dma(&mut channel, &mut capture)!
        session.start(128)!
        session.finish()!
        first := session.buffer()![0]
    }
    ring: [1024]u16 = [0; 1024]
    {
        stream := sampler.stream(&mut channel, &mut ring)!
        stream.read(&mut samples)!
        size := stream.ring_size()
    }
    b.dma.release(channel)
    b.pins.release_sampler(sampler)
    knob := b.pins.analog(26)!
    reading := knob.read()!
    b.pins.release_analog(knob)
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        for level in ["0", "2"] {
            let object = scratch.path(&format!("sampler-{board}-O{level}.o"));
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

#[test]
fn adc_signals_are_raised_from_the_default_adc_interrupt() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("adc");
    let example = root().join("examples/adc.dodo");
    // ADC_IRQ_FIFO's vector: IRQ 22 on the RP2040, 35 on the RP2350.
    let boards = [
        ("pico", 0xE48B_FF56, 22),
        ("pico2", 0xE48B_FF59, 35),
        ("pico2_w", 0xE48B_FF59, 35),
    ];
    for (board, family, irq) in boards {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("adc-{board}-O{level}.elf"));
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
            let serve = elf_symbol(&bytes, "dodo_adc_interrupt").expect("the handler is linked");
            assert_eq!(serve & 1, 1, "{board} -O{level}: a Thumb function");
            assert_eq!(
                stub_literal(Path::new(&elf), family, "AdcHandler", 10),
                serve
            );
            let image = flash_image(
                &fs::read(Path::new(&elf).with_extension("uf2")).unwrap(),
                family,
            );
            let vectors = elf_symbol(&bytes, "__vector_table").unwrap() as usize - 0x1000_0000;
            assert_eq!(
                word(&image, vectors + 4 * (16 + irq)),
                elf_symbol(&bytes, "AdcHandler").unwrap() | 1,
                "{board} -O{level}: the vector is the runtime's handler"
            );
        }
    }
    // A program's own handler replaces the runtime's.
    let own = scratch.write(
        "own.dodo",
        "package main\nimport \"std/embedded/board\"\nstatic mut COUNT: u32 = 0\n@interrupt(ADC_IRQ_FIFO)\nfn adc() {\n unsafe {\n  COUNT += 1\n }\n}\nfn main() {\n b := board.take()!\n b.led.toggle()\n}\n",
    );
    for (board, family, irq) in [("pico", 0xE48B_FF56, 22), ("pico2", 0xE48B_FF59, 35)] {
        let elf = scratch.path(&format!("own-{board}.elf"));
        build(&["build", own.to_str().unwrap(), "--board", board, "-o", &elf]);
        let bytes = fs::read(&elf).unwrap();
        let image = flash_image(
            &fs::read(Path::new(&elf).with_extension("uf2")).unwrap(),
            family,
        );
        let vectors = elf_symbol(&bytes, "__vector_table").unwrap() as usize - 0x1000_0000;
        let handler = elf_symbol(&bytes, "ADC_IRQ_FIFO").expect("the program's handler");
        assert_ne!(Some(handler), elf_symbol(&bytes, "AdcHandler"), "{board}");
        assert_eq!(
            word(&image, vectors + 4 * (16 + irq)),
            handler | 1,
            "{board}"
        );
    }
    // Without std/embedded/chip the stub falls through to DefaultHandler.
    let bare = scratch.write("bare.dodo", "package main\nfn main() {}\n");
    for (chip, family) in [("rp2040", 0xE48B_FF56), ("rp2350", 0xE48B_FF59)] {
        let elf = scratch.path(&format!("bare-{chip}.elf"));
        build(&["build", bare.to_str().unwrap(), "--chip", chip, "-o", &elf]);
        assert_eq!(
            stub_literal(Path::new(&elf), family, "AdcHandler", 10),
            0,
            "{chip}"
        );
    }
}

#[test]
fn random_number_generators_compile_for_every_board() {
    let scratch = Scratch::new("rng-compile");
    let source = scratch.write(
        "rng.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/hal"
fn roll<R: hal.Rng>(rng: &mut R) -> u32!hal.Error {
    return ok(hal.random_below(rng, 6)? + 1)
}
fn main() {
    b := board.take()!
    key := [0u8; 32]
    b.rng.fill(&mut key)!
    soft := hal.SoftRng.from_rng(&mut b.rng)!
    b.timer.delay_us(soft.next_u32() % 1000)
    generator: &mut dyn hal.Rng = &mut b.rng
    generator.fill(&mut key)!
    if roll(&mut b.rng)! == 6 && hal.random_u64(&mut soft)! != 0 {
        b.led.toggle()
    }
}
"#,
    );
    for board in ["pico", "pico2", "pico2_w"] {
        for level in ["0", "2"] {
            let object = scratch.path(&format!("rng-{board}-O{level}.o"));
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

fn check_esp32c6_firmware(elf: &Path) {
    let bytes = fs::read(elf).unwrap();
    let image = fs::read(elf.with_extension("bin")).unwrap();
    // RISC-V, entered by the boot ROM's direct boot at flash + 8, after the
    // marker, which is the ELF's entry.
    assert_eq!(u16::from_le_bytes([bytes[0x12], bytes[0x13]]), 0xF3);
    assert_eq!(word(&image, 0), 0xAEDB_041D);
    assert_eq!(word(&image, 4), 0xAEDB_041D);
    assert_eq!(word(&bytes, 0x18), 0x4200_0008);
    assert_eq!(elf_symbol(&bytes, "Reset_Handler"), Some(0x4200_0008));
    // The vectored trap table mtvec needs, and the routed sources' table.
    let vector = elf_symbol(&bytes, "__vector_table").expect("trap vector");
    assert_eq!(vector % 256, 0);
    assert!(elf_symbol(&bytes, "dodo_interrupt_table").is_some());
    // The flash setup runs from SRAM, where the image's .data puts it.
    let setup = elf_symbol(&bytes, "dodo_esp32c6_flash_setup").expect("flash setup");
    assert!((0x4080_0000..0x4088_0000).contains(&setup), "{setup:#x}");
    assert!(image.len() < 8 * 1024 * 1024);
}

fn build_esp32c6(args: &[&str]) {
    let output = dodo(args);
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{args:?}: {message}");
    assert!(message.contains(".bin"), "{message}");
}

#[test]
fn esp32c6_projects_link_to_flashable_firmware() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("link-esp32c6");
    let blink = root().join("examples/blink");
    let tasks = root().join("examples/esp32c6_tasks");
    let selftest = root().join("tests/hardware/esp32c6_devkitc");
    let can_audio = root().join("tests/hardware/esp32c6_can_audio");
    for (project, name, board) in [
        (&blink, "blink", &["--board", "esp32c6_devkitc"][..]),
        (&tasks, "tasks", &[][..]),
        (&selftest, "selftest", &[][..]),
        (&can_audio, "can-audio", &[][..]),
    ] {
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("{name}-O{level}.elf"));
            let mut args = vec!["build", project.to_str().unwrap(), "-O", level, "-o", &elf];
            args.extend(board);
            build_esp32c6(&args);
            check_esp32c6_firmware(Path::new(&elf));
        }
    }
    // The portable examples, unchanged: drivers tested with fakes on the
    // board's buses, an ADC input and a PWM output, displays, and the
    // serial consoles.
    for name in [
        "hal_sensor_board",
        "dimmer_board",
        "thermometer_board",
        "oled_demo",
        "serial_console",
        "usb_serial",
    ] {
        let source = root().join(format!("examples/{name}.dodo"));
        for level in ["0", "2"] {
            let elf = scratch.path(&format!("{name}-O{level}.elf"));
            build_esp32c6(&[
                "build",
                source.to_str().unwrap(),
                "--board",
                "esp32c6_devkitc",
                "-O",
                level,
                "-o",
                &elf,
            ]);
            check_esp32c6_firmware(Path::new(&elf));
        }
    }
    // A custom board with only `chip` set, and the default panic hook's
    // reporter, which the chip package defines.
    let custom = scratch.write(
        "custom.dodo",
        "package main\nimport \"std/embedded/chip\"\nfn main() {\n p := chip.take()!\n led := p.pins.output(15)!\n led.set_high()\n}\n",
    );
    let elf = scratch.path("custom.elf");
    build_esp32c6(&[
        "build",
        custom.to_str().unwrap(),
        "--chip",
        "esp32c6",
        "-o",
        &elf,
    ]);
    check_esp32c6_firmware(Path::new(&elf));
    let bytes = fs::read(&elf).unwrap();
    assert!(elf_symbol(&bytes, "dodo_panic_report").is_some());
    assert!(elf_symbol(&bytes, "dodo_board_panic").is_some());
}

// The ESP32-C6 runtime saves the registers and calls `@interrupt` handlers
// as ordinary functions, so they return with `ret`, not `mret`, and names
// come from the chip's interrupt sources.
#[test]
fn esp32c6_interrupt_handlers_are_plain_functions() {
    let scratch = Scratch::new("esp32c6-interrupts");
    let source = scratch.write(
        "main.dodo",
        "package main\nimport \"std/embedded/chip\"\nstatic mut COUNT: u32 = 0\n@interrupt(GPIO)\nfn edge() {\n    unsafe {\n        COUNT += 1\n    }\n}\nfn main() {\n    chip.enable(chip.GPIO)\n}\n",
    );
    let assembly = scratch.path("main.s");
    stdout(&dodo(&[
        "build",
        source.to_str().unwrap(),
        "--board",
        "esp32c6_devkitc",
        "-O",
        "2",
        "--emit",
        "asm",
        "-o",
        &assembly,
    ]));
    let text = fs::read_to_string(&assembly).unwrap();
    let handler = text.split("\nGPIO:").nth(1).expect("exported handler");
    let body = handler.split(".Lfunc_end").next().unwrap();
    assert!(text.contains(".globl\tGPIO"), "{text}");
    assert!(body.contains("\tret") && !body.contains("mret"), "{body}");

    let wrong = scratch.write(
        "wrong.dodo",
        "package main\n@interrupt(UART0_IRQ)\nfn serial() {}\nfn main() {}\n",
    );
    let error = stderr(&dodo(&[
        "check",
        wrong.to_str().unwrap(),
        "--board",
        "esp32c6_devkitc",
    ]));
    assert!(
        error.contains("unknown interrupt vector `UART0_IRQ` for this chip"),
        "{error}"
    );
}

// Board settings, atomics with the A extension, and the chip's packages.
#[test]
fn esp32c6_settings_and_packages_check() {
    let blink = root().join("examples/blink");
    let config = stdout(&dodo(&[
        "build",
        blink.to_str().unwrap(),
        "--board",
        "esp32c6_devkitc",
        "--print-config",
    ]));
    for line in [
        "triple = \"riscv32-unknown-none-elf\" # board esp32c6_devkitc",
        "cpu = \"generic-rv32\" # board esp32c6_devkitc",
        "features = \"+m,+a,+c\" # board esp32c6_devkitc",
        "panic-hook = \"dodo_board_panic\" # board esp32c6_devkitc",
    ] {
        assert!(config.contains(line), "{line}\n{config}");
    }
    let scratch = Scratch::new("esp32c6-check");
    let source = scratch.write(
        "main.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/chip"
import "std/embedded/hal"
import "std/sync/atomic"
static HITS: atomic.Atomic<u32> = atomic.Atomic.new(0u32)
fn main() {
    b := board.take()!
    HITS.fetch_add(1, atomic.Ordering.AcqRel)!
    i2c := b.pins.i2c(board.I2C_SDA_PIN, board.I2C_SCL_PIN, hal.I2C_FAST_HZ)!
    spi := b.pins.spi(board.SPI_CLOCK_PIN, board.SPI_TX_PIN, some(board.SPI_RX_PIN), hal.SpiMode.Mode0, 8_000_000)!
    serial := b.pins.uart(board.UART_TX_PIN, some(board.UART_RX_PIN), 115_200)!
    output := b.pins.pwm(10, 50)!
    hal.set_pulse_us(&mut output, 1500, 20_000)!
    input := b.pins.analog(2)!
    millivolts := hal.read_millivolts(&mut input, 3300)!
    sensor := b.pins.temperature()!
    celsius := chip.millicelsius(sensor.read()!) / 1000
    channel := b.dma.channel()!
    source: [4]u8 = [1, 2, 3, 4]
    target: [4]u8 = [0; 4]
    channel.copy(&mut target, &source)!
    usb := b.usb.serial()!
    b.led.set_color(0x00_10_00)
    core.assert(millivolts <= 3300 && celsius > -40)
    b.pins.release_i2c(i2c)
    b.pins.release_spi(spi)
    b.pins.release_uart(serial)
    b.pins.release_pwm(output)
    b.pins.release_analog(input)
    b.pins.release_analog(sensor)
    b.dma.release(channel)
    b.usb.release_serial(usb)
}
"#,
    );
    stdout(&dodo(&[
        "check",
        source.to_str().unwrap(),
        "--board",
        "esp32c6_devkitc",
    ]));
    let object = scratch.path("main.o");
    stdout(&dodo(&[
        "build",
        source.to_str().unwrap(),
        "--board",
        "esp32c6_devkitc",
        "--emit",
        "obj",
        "-o",
        &object,
    ]));
}

#[test]
fn avr_runtime_provides_startup_and_helper_symbols() {
    let scratch = Scratch::new("avr-runtime");
    let chip = dodoc::hardware::chip("atmega328p").unwrap();
    let object = scratch.0.join("atmega328p.o");
    let options = dodoc::codegen::Options {
        target: Some(chip.triple.into()),
        cpu: Some(chip.cpu.into()),
        optimization: 2,
        ..Default::default()
    };
    dodoc::codegen::compile_ir_object(&options, chip.name, &(chip.runtime)(), &object).unwrap();
    let bytes = fs::read(&object).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    for name in [
        ".vectors",
        "__vectors",
        "__reset",
        "__do_copy_data",
        "__do_clear_bss",
        "__bad_interrupt",
        chip.panic_hook,
        "dodo_panic_report",
        "abort",
        "TIMER0_OVF",
        "USART_RX",
        "dodo_avr_ticks",
        "dodo_avr_delay_loops",
        "dodo_avr_flash_byte",
        "dodo_avr_rx_take",
        "dodo_avr_eeprom_write",
        "dodo_avr_watchdog",
        "memcpy",
        "memmove",
        "memset",
        "memcmp",
        "__mulsi3",
        "__muldi3",
        "__mulosi4",
        "__mulodi4",
        "__udivmodqi4",
        "__divmodhi4",
        "__udivmodsi4",
        "__udivdi3",
        "__moddi3",
        "__addsf3",
        "__divsf3",
        "__ltsf2",
        "__unordsf2",
        "__fixsfsi",
        "__floatundisf",
        "__adddf3",
        "__divdf3",
        "__gedf2",
        "__extendsfdf2",
        "__sync_val_compare_and_swap_2",
        "__sync_lock_test_and_set_1",
        "__sync_fetch_and_add_4",
    ] {
        assert!(text.contains(name), "atmega328p runtime lacks {name}");
    }
}

// Function values are word addresses in AVR program memory (address space
// 1), interrupt handlers use the signal ABI, panic messages stay in flash,
// and size counts more than speed.
#[test]
fn avr_code_generation_keeps_code_and_constants_in_program_memory() {
    let scratch = Scratch::new("avr-codegen");
    let source = scratch.write(
        "main.dodo",
        r#"package main
import "std/embedded/board"
import "std/embedded/hal"
static mut COUNT: u16 = 0
fn triple(value: u32) -> u32 {
    return value * 3
}
fn apply(function: fn(u32) -> u32, value: u32) -> u32 {
    return function(value)
}
fn call(function: &dyn fn(u32) -> u32, value: u32) -> u32 {
    return function(value)
}
fn drive(pin: &mut dyn hal.OutputPin) {
    pin.set_high()
}
const TABLE: [1]fn(u32) -> u32 = [triple]
@interrupt(TIMER1_COMPA)
fn tick() {
    unsafe {
        COUNT += 1
    }
}
fn main() {
    b := board.take()!
    offset := apply(TABLE[0], 2) + call(triple, 1)
    drive(&mut b.led)
    total := call(fn(x) {
        x + offset
    }, 4)
    values: [2]u32 = [total, 0]
    b.timer.delay_ms(values[(total & 1) as usize] + 1)
}
"#,
    );
    let source = source.to_str().unwrap();
    for level in ["0", "2"] {
        let ir = scratch.path(&format!("main-O{level}.ll"));
        stdout(&dodo(&[
            "build",
            source,
            "--board",
            "arduino_uno",
            "-O",
            level,
            "--emit",
            "llvm-ir",
            "-o",
            &ir,
        ]));
        let ir = fs::read_to_string(&ir).unwrap();
        assert!(ir.contains("target triple = \"avr-none\""), "{ir}");
        if level == "0" {
            // Unoptimized, the function table, the closure pair and the
            // function-to-closure conversion are all still there.
            assert!(
                ir.contains("[1 x ptr addrspace(1)] [ptr addrspace(1) @dodo.main.triple]"),
                "{ir}"
            );
            assert!(ir.contains("{ ptr addrspace(1), ptr }"), "{ir}");
            assert!(
                ir.contains("ptr inttoptr (i16 ptrtoint (ptr addrspace(1) @dodo.main.triple"),
                "{ir}"
            );
        }
        assert!(ir.contains("addrspace(1) constant"), "panic text: {ir}");
        assert!(ir.contains("section \".progmem.data."), "{ir}");
        assert!(!ir.contains("section \".rodata.panic"), "{ir}");
        let handler = ir
            .lines()
            .find(|line| line.starts_with("define") && line.contains("@TIMER1_COMPA"))
            .unwrap();
        let group = handler
            .split('#')
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap();
        let attributes = ir
            .lines()
            .find(|line| line.starts_with(&format!("attributes #{group} ")))
            .unwrap();
        assert!(attributes.contains("\"signal\""), "{attributes}");
        assert!(attributes.contains("optsize"), "{attributes}");
        let object = scratch.path(&format!("main-O{level}.o"));
        stdout(&dodo(&[
            "build",
            source,
            "--board",
            "arduino_uno",
            "-O",
            level,
            "--emit",
            "obj",
            "-o",
            &object,
        ]));
    }
    // Nothing else sets the optimization level, so the board picks 2;
    // explicit settings still win.
    let config = stdout(&dodo(&[
        "build",
        source,
        "--board",
        "arduino_uno",
        "--print-config",
    ]));
    for line in [
        "triple = \"avr-none\" # board arduino_uno",
        "cpu = \"atmega328p\" # board arduino_uno",
        "opt-level = 2 # board arduino_uno",
        "panic-hook = \"dodo_board_panic\" # board arduino_uno",
    ] {
        assert!(config.contains(line), "missing {line:?} in\n{config}");
    }
    let config = stdout(&dodo(&[
        "build",
        source,
        "--board",
        "arduino_uno",
        "-O",
        "0",
        "--print-config",
    ]));
    assert!(config.contains("opt-level = 0 # CLI"), "{config}");
    let config = stdout(&dodo(&[
        "build",
        source,
        "--board",
        "arduino_uno",
        "--release",
        "--print-config",
    ]));
    assert!(
        config.contains("opt-level = 3 # built-in release profile"),
        "{config}"
    );
}

/// The flash contents an Intel HEX file describes, gaps erased.
fn intel_hex_image(text: &str) -> Vec<u8> {
    let mut image = vec![];
    let mut upper = 0usize;
    for line in text.lines() {
        let bytes: Vec<u8> = (1..line.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&line[at..at + 2], 16).unwrap())
            .collect();
        assert_eq!(
            bytes.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)),
            0,
            "{line}"
        );
        let count = bytes[0] as usize;
        let address = u16::from_be_bytes([bytes[1], bytes[2]]) as usize;
        let data = &bytes[4..4 + count];
        match bytes[3] {
            0 => {
                let at = upper + address;
                if image.len() < at + count {
                    image.resize(at + count, 0xFF);
                }
                image[at..at + count].copy_from_slice(data);
            }
            1 => return image,
            4 => upper = (u16::from_be_bytes([data[0], data[1]]) as usize) << 16,
            kind => panic!("record type {kind}"),
        }
    }
    panic!("no end-of-file record");
}

/// A linked ATmega328P program: its Intel HEX image starts with the vector
/// table's jmp instructions, fits below the Uno's bootloader, and holds
/// `.data`'s load image, whose RAM copy lies in the 2 KiB of SRAM.
fn check_atmega328p_firmware(elf: &Path) -> Vec<u8> {
    let bytes = fs::read(elf).unwrap();
    assert_eq!(&bytes[..4], b"\x7fELF");
    assert_eq!(u16::from_le_bytes([bytes[18], bytes[19]]), 83, "EM_AVR");
    let image = intel_hex_image(&fs::read_to_string(elf.with_extension("hex")).unwrap());
    assert!(image.len() <= 32 * 1024 - 512, "{} bytes", image.len());
    for vector in 0..26 {
        let word = u16::from_le_bytes([image[4 * vector], image[4 * vector + 1]]);
        assert_eq!(word & 0xFE0E, 0x940C, "vector {vector} is not a jmp");
    }
    let start = elf_symbol(&bytes, "__data_start").unwrap();
    let end = elf_symbol(&bytes, "__data_end").unwrap();
    let load = elf_symbol(&bytes, "__data_load").unwrap() as usize;
    assert!((0x80_0100..=0x80_0900).contains(&start) && end >= start);
    assert_eq!(load + (end - start) as usize, image.len().max(load));
    image
}

#[test]
fn arduino_uno_projects_link_to_flashable_firmware() {
    if !ld_lld() {
        return;
    }
    let scratch = Scratch::new("uno");
    // Board-generic examples, unchanged, and the hardware self-test.
    for (source, args, name) in [
        ("examples/blink", &[][..], "blink"),
        ("examples/serial_console.dodo", &[][..], "console"),
        ("examples/usb_serial.dodo", &[][..], "usb"),
        ("examples/dimmer_board.dodo", &[][..], "dimmer"),
        ("examples/hal_sensor_board.dodo", &[][..], "sensor"),
        ("examples/uno_boot_counter.dodo", &[][..], "counter"),
        ("tests/hardware/arduino_uno", &["-b", "core"][..], "core"),
        ("tests/hardware/arduino_uno", &["-b", "wide"][..], "wide"),
        (
            "tests/hardware/arduino_uno",
            &["-b", "peripherals"][..],
            "peripherals",
        ),
    ] {
        let elf = scratch.path(&format!("{name}.elf"));
        let source = root().join(source);
        // Manifest targets name the board themselves.
        let mut command = vec!["build", source.to_str().unwrap()];
        if args.is_empty() {
            command.extend_from_slice(&["--board", "arduino_uno"]);
        }
        command.extend_from_slice(args);
        command.extend_from_slice(&["-o", &elf]);
        let output = dodo(&command);
        let message = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "{command:?}: {message}");
        assert!(message.contains(".hex"), "{message}");
        let image = check_atmega328p_firmware(Path::new(&elf));
        if name == "console" {
            // Panic messages are in flash, outside `.data`'s load image.
            let bytes = fs::read(&elf).unwrap();
            let load = elf_symbol(&bytes, "__data_load").unwrap() as usize;
            let text = b"index bounds\0";
            let at = image
                .windows(text.len())
                .position(|window| window == text)
                .expect("panic text in flash");
            assert!(at < load, "panic text at {at:#x}, .data from {load:#x}");
        }
    }
    // A custom board with only `chip` set gets all 32 KiB.
    let custom = scratch.write(
        "custom.dodo",
        "package main\nimport \"std/embedded/chip\"\nfn main() {\n p := chip.take(chip.Config { cpu_hz: 8_000_000 })!\n led := p.pins.output(chip.PB0)!\n led.set_high()\n}\n",
    );
    let elf = scratch.path("custom.elf");
    let output = dodo(&[
        "build",
        custom.to_str().unwrap(),
        "--chip",
        "atmega328p",
        "-o",
        &elf,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    check_atmega328p_firmware(Path::new(&elf));
    // Constants live in SRAM on AVR: too many fail at the link, naming the
    // stack they would leave too little room for.
    let large = scratch.write(
        "large.dodo",
        "package main\nimport \"std/embedded/board\"\nstatic TABLE: [30000]u8 = [1u8; 30000]\nfn main() {\n b := board.take()!\n index := b.timer.now_us() as usize % 30000\n b.timer.delay_ms(TABLE[index] as u32)\n}\n",
    );
    let error = stderr(&dodo(&[
        "build",
        large.to_str().unwrap(),
        "--board",
        "arduino_uno",
        "-o",
        &scratch.path("large.elf"),
    ]));
    assert!(
        error.contains("RAM: statics and constants leave less than 256 bytes for the stack"),
        "{error}"
    );
}
