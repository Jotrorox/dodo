//! Runs Dodo firmware for the Raspberry Pi RP2040 or RP2350 in an emulator
//! and reports how it ended.
//!
//! The firmware is a linked ELF from `dodo build --board pico` (RP2040) or
//! `--board pico2` (RP2350). It boots as the boot ROM would start it, from
//! the vector table in flash, with the real boot ROM mapped so that ROM
//! functions resolve and core 1 waits to be launched. The run ends
//! when the firmware's result record says it finished or panicked, when a
//! core lands in the runtime's DefaultHandler (an unhandled fault or
//! interrupt), or after a cycle budget.
//!
//! The record is a static whose symbol name ends in `.EMULATOR_RESULT`, as in
//! tests/emulator/selftest.dodo:
//!
//!   [0] 0x5747_00ss running step ss, 0x600D_00ss finished after ss steps,
//!       0xDEAD_0000 panicked
//!   [1] line  [2] column  [3] check message  [4] file name (C strings)
//!
//! The emulators are the rp2040-emu and rp2350-emu crates of the picoem
//! project, with the fixes in patches/. scripts/test_emulator.py prepares
//! them, builds this runner, and drives it; see README.md.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "usage: dodo-emulator --bootrom PATH [--max-cycles N] FIRMWARE.elf";
const FLASH: u32 = 0x1000_0000;
const FLASH_END: u32 = 0x1100_0000;
const RUNNING: u32 = 0x5747;
const FINISHED: u32 = 0x600D;
const PANICKED: u32 = 0xDEAD;
// Cycles between checks of the record and the cores: well under a
// millisecond of emulated time, so a run stops promptly once it ends.
const CHUNK: u64 = 20_000;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("dodo-emulator: {message}");
            ExitCode::from(2)
        }
    }
}

struct Options {
    firmware: PathBuf,
    bootrom: PathBuf,
    max_cycles: u64,
}

fn options() -> Result<Options, String> {
    let mut firmware = None;
    let mut bootrom = None;
    // About a second of emulated time: the self-tests take milliseconds.
    let mut max_cycles = 150_000_000u64;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bootrom" => bootrom = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            "--max-cycles" => {
                let value = args.next().ok_or(USAGE)?;
                max_cycles = value
                    .replace('_', "")
                    .parse()
                    .map_err(|_| format!("invalid cycle count {value:?}"))?;
            }
            "-h" | "--help" => return Err(USAGE.to_owned()),
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}\n{USAGE}")),
            _ if firmware.is_none() => firmware = Some(PathBuf::from(arg)),
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(Options {
        firmware: firmware.ok_or(USAGE)?,
        bootrom: bootrom.ok_or(USAGE)?,
        max_cycles,
    })
}

fn run() -> Result<bool, String> {
    let options = options()?;
    let read = |path: &PathBuf| {
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
    };
    let elf = Elf::parse(read(&options.firmware)?)
        .map_err(|error| format!("{}: {error}", options.firmware.display()))?;
    let bootrom = read(&options.bootrom)?;
    // A boot ROM ELF (the RP2040's is published as one) holds the ROM at 0.
    let bootrom = if bootrom.starts_with(b"\x7fELF") {
        Elf::parse(bootrom)
            .map_err(|error| format!("{}: {error}", options.bootrom.display()))?
            .image(0, 0x1_0000)
    } else {
        bootrom
    };
    let chip = if elf.symbol("__picobin_block").is_some() {
        Chip::Rp2350
    } else if elf.symbol("__boot2_start").is_some() {
        Chip::Rp2040
    } else {
        return Err("not RP2040 or RP2350 firmware: no boot stage 2 or IMAGE_DEF block".into());
    };
    let record = elf
        .symbols
        .iter()
        .find(|symbol| symbol.name.ends_with(".EMULATOR_RESULT"))
        .map(|symbol| symbol.value)
        .ok_or("the firmware has no EMULATOR_RESULT record")?;
    let default_handler = elf.symbol("DefaultHandler").map(|symbol| symbol.value & !1);
    let flash = elf.image(FLASH, FLASH_END);
    let mut machine: Box<dyn Machine> = match chip {
        Chip::Rp2040 => Box::new(Rp2040::boot(&bootrom, &flash)?),
        Chip::Rp2350 => Box::new(Rp2350::boot(&bootrom, &flash)?),
    };

    let started = Instant::now();
    let summary = |machine: &dyn Machine| {
        let cycles = machine.cycles();
        format!(
            "{cycles} cycles, {:.1} ms at {} MHz, {:.2} s on the host",
            cycles as f64 / chip.mhz() as f64 / 1000.0,
            chip.mhz(),
            started.elapsed().as_secs_f64()
        )
    };
    loop {
        let before = machine.cycles();
        machine.run(CHUNK)?;
        let ran = machine.cycles() - before;
        let state = machine.peek(record);
        match state >> 16 {
            FINISHED => {
                println!(
                    "{}: finished {} steps ({})",
                    chip.name(),
                    state & 0xFFFF,
                    summary(&*machine)
                );
                return Ok(true);
            }
            PANICKED => {
                let line = machine.peek(record + 4);
                let column = machine.peek(record + 8);
                let message = elf.c_string(machine.peek(record + 12));
                let file = elf.c_string(machine.peek(record + 16));
                println!(
                    "{}: panicked at {file}:{line}:{column}: {message} ({})",
                    chip.name(),
                    summary(&*machine)
                );
                if let Some(source) = source_line(&file, line) {
                    println!("    {source}");
                }
                return Ok(false);
            }
            _ => {}
        }
        let step = if state >> 16 == RUNNING {
            format!("in step {}", state & 0xFFFF)
        } else {
            "before the first step".to_owned()
        };
        for core in 0..2 {
            let registers = machine.core(core);
            if Some(registers.pc & !1) == default_handler && !registers.halted {
                println!(
                    "{}: core {core} {step} took an unhandled {} ({})",
                    chip.name(),
                    fault(&elf, &*machine, &registers),
                    summary(&*machine)
                );
                return Ok(false);
            }
        }
        if ran == 0 || machine.cycles() >= options.max_cycles {
            let mut report = String::new();
            for core in 0..2 {
                let registers = machine.core(core);
                let state = if registers.halted { ", halted" } else { "" };
                let _ = write!(
                    report,
                    "\n    core {core}: pc {:#010x} {}, lr {:#010x} {}{state}",
                    registers.pc,
                    elf.describe(registers.pc),
                    registers.lr,
                    elf.describe(registers.lr)
                );
            }
            let reason = if ran == 0 { "stopped" } else { "timed out" };
            println!(
                "{}: {reason} {step} ({}){report}",
                chip.name(),
                summary(&*machine)
            );
            return Ok(false);
        }
    }
}

// The exception a core took, and where: the stacked return address of the
// interrupted code.
fn fault(elf: &Elf, machine: &dyn Machine, registers: &Registers) -> String {
    let exception = registers.xpsr & 0x1FF;
    let name = match exception {
        2 => "NMI".to_owned(),
        3 => "HardFault".to_owned(),
        4 => "MemManage fault".to_owned(),
        5 => "BusFault".to_owned(),
        6 => "UsageFault".to_owned(),
        7 => "SecureFault".to_owned(),
        11 => "SVCall".to_owned(),
        14 => "PendSV".to_owned(),
        15 => "SysTick".to_owned(),
        16.. => format!("interrupt {}", exception - 16),
        _ => format!("exception {exception}"),
    };
    let stacked_lr = machine.peek(registers.sp.wrapping_add(20));
    let stacked_pc = machine.peek(registers.sp.wrapping_add(24));
    format!(
        "{name} from pc {stacked_pc:#010x} {}, lr {stacked_lr:#010x} {}",
        elf.describe(stacked_pc),
        elf.describe(stacked_lr)
    )
}

fn source_line(file: &str, line: u32) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let line = text.lines().nth(line.checked_sub(1)? as usize)?;
    Some(line.trim().to_owned())
}

#[derive(Clone, Copy)]
enum Chip {
    Rp2040,
    Rp2350,
}

impl Chip {
    fn name(self) -> &'static str {
        match self {
            Chip::Rp2040 => "rp2040",
            Chip::Rp2350 => "rp2350",
        }
    }

    // clk_sys as std/embedded/chip configures it.
    fn mhz(self) -> u32 {
        match self {
            Chip::Rp2040 => 125,
            Chip::Rp2350 => 150,
        }
    }
}

struct Registers {
    pc: u32,
    lr: u32,
    sp: u32,
    xpsr: u32,
    halted: bool,
}

trait Machine {
    fn run(&mut self, cycles: u64) -> Result<(), String>;
    fn cycles(&self) -> u64;
    fn peek(&self, address: u32) -> u32;
    fn core(&self, core: usize) -> Registers;
}

struct Rp2040(rp2040_emu::Emulator);

impl Rp2040 {
    fn boot(bootrom: &[u8], flash: &[u8]) -> Result<Self, String> {
        let mut emulator = rp2040_emu::EmulatorBuilder::new(Default::default())
            .build()
            .map_err(|error| format!("cannot start the RP2040 emulator: {error:?}"))?;
        emulator.load_bootrom(bootrom);
        emulator.load_flash(flash);
        emulator.reset();
        // What boot stage 2 does after configuring the flash interface: set
        // the stack pointer, VTOR, and the program counter from the vector
        // table at flash + 0x100, with core 1 left to wait for its launch.
        // The emulator does not model the flash interface that stage 2 sets
        // up, so the boot ROM's own check of stage 2 cannot pass.
        emulator.direct_boot_from_flash(0x100);
        Ok(Self(emulator))
    }
}

impl Machine for Rp2040 {
    fn run(&mut self, cycles: u64) -> Result<(), String> {
        self.0
            .run(cycles)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    }
    fn cycles(&self) -> u64 {
        self.0.cycles()
    }
    fn peek(&self, address: u32) -> u32 {
        self.0.peek(address)
    }
    fn core(&self, core: usize) -> Registers {
        let core = self.0.core(core);
        Registers {
            pc: core.regs.pc(),
            lr: core.regs.r[14],
            sp: core.regs.sp(),
            xpsr: core.regs.xpsr,
            halted: core.is_halted(),
        }
    }
}

struct Rp2350(rp2350_emu::Emulator);

impl Rp2350 {
    fn boot(bootrom: &[u8], flash: &[u8]) -> Result<Self, String> {
        if flash.len() < 8 {
            return Err("the firmware has no vector table".into());
        }
        let mut emulator = rp2350_emu::EmulatorBuilder::new(Default::default())
            .build()
            .map_err(|error| format!("cannot start the RP2350 emulator: {error:?}"))?;
        emulator.load_bootrom(bootrom);
        emulator.load_flash(flash);
        emulator.reset();
        // Core 0 enters the image as the boot ROM would after finding its
        // IMAGE_DEF block: stack pointer and reset handler from the vector
        // table at the start of flash. Core 1 runs the boot ROM from reset,
        // which parks it until the firmware launches it.
        let word =
            |offset: usize| u32::from_le_bytes(flash[offset..offset + 4].try_into().unwrap());
        let core = emulator.core_mut(0);
        core.regs.msp = word(0);
        core.regs.r[13] = word(0);
        core.regs.set_pc(word(4) & !1);
        core.ppb.vtor = FLASH;
        // Core 0's boot path seeds both cores' redundancy coprocessor salt;
        // core 1 waits in the ROM until it has one.
        emulator.bus.atomics.rcp_salt_set(0, 0x5EED_0000);
        emulator.bus.atomics.rcp_salt_set(1, 0x5EED_0001);
        Ok(Self(emulator))
    }
}

impl Machine for Rp2350 {
    fn run(&mut self, cycles: u64) -> Result<(), String> {
        self.0
            .run(cycles)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    }
    fn cycles(&self) -> u64 {
        self.0.cycles()
    }
    fn peek(&self, address: u32) -> u32 {
        self.0.peek(address)
    }
    fn core(&self, core: usize) -> Registers {
        let core = self.0.core(core);
        Registers {
            pc: core.regs.pc(),
            lr: core.regs.r[14],
            sp: core.regs.sp(),
            xpsr: core.regs.xpsr,
            halted: core.is_halted(),
        }
    }
}

struct Symbol {
    name: String,
    value: u32,
    size: u32,
    function: bool,
}

struct Segment {
    address: u32,
    data: std::ops::Range<usize>,
}

// The parts of a 32-bit little-endian ELF file the runner needs: the loaded
// segments, by load address, and the symbol table.
struct Elf {
    bytes: Vec<u8>,
    segments: Vec<Segment>,
    symbols: Vec<Symbol>,
}

impl Elf {
    fn parse(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() < 52 || !bytes.starts_with(b"\x7fELF\x01\x01") {
            return Err("not a 32-bit little-endian ELF file".into());
        }
        let u16_at = |offset: usize| -> Result<u32, String> {
            bytes
                .get(offset..offset + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]) as u32)
                .ok_or_else(|| "truncated ELF file".to_owned())
        };
        let u32_at = |offset: usize| -> Result<u32, String> {
            bytes
                .get(offset..offset + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .ok_or_else(|| "truncated ELF file".to_owned())
        };
        let mut segments = Vec::new();
        let (phoff, phentsize, phnum) = (u32_at(28)?, u16_at(42)?, u16_at(44)?);
        for index in 0..phnum {
            let header = (phoff + index * phentsize) as usize;
            // PT_LOAD, by physical (load) address and file contents.
            if u32_at(header)? != 1 {
                continue;
            }
            let (offset, paddr, filesz) = (
                u32_at(header + 4)?,
                u32_at(header + 12)?,
                u32_at(header + 16)?,
            );
            let data = offset as usize..(offset + filesz) as usize;
            if data.end > bytes.len() {
                return Err("truncated ELF segment".into());
            }
            segments.push(Segment {
                address: paddr,
                data,
            });
        }
        let mut symbols = Vec::new();
        let (shoff, shentsize, shnum) = (u32_at(32)?, u16_at(46)?, u16_at(48)?);
        for index in 0..shnum {
            let header = (shoff + index * shentsize) as usize;
            // SHT_SYMTAB, with its string table in the linked section.
            if u32_at(header + 4)? != 2 {
                continue;
            }
            let (offset, size, link, entsize) = (
                u32_at(header + 16)?,
                u32_at(header + 20)?,
                u32_at(header + 24)?,
                u32_at(header + 36)?,
            );
            let strings = (shoff + link * shentsize) as usize;
            let strings = u32_at(strings + 16)? as usize;
            for entry in (offset..offset + size).step_by(entsize.max(16) as usize) {
                let entry = entry as usize;
                let name = strings + u32_at(entry)? as usize;
                let name = bytes.get(name..).unwrap_or_default();
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
                let info = *bytes.get(entry + 12).ok_or("truncated ELF symbol")?;
                symbols.push(Symbol {
                    name: String::from_utf8_lossy(name).into_owned(),
                    value: u32_at(entry + 4)?,
                    size: u32_at(entry + 8)?,
                    function: info & 0xF == 2,
                });
            }
        }
        Ok(Self {
            bytes,
            segments,
            symbols,
        })
    }

    fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| symbol.name == name)
    }

    // The loaded bytes between `start` and `end`, from address `start`,
    // zero-filled between segments.
    fn image(&self, start: u32, end: u32) -> Vec<u8> {
        let mut image = Vec::new();
        for segment in &self.segments {
            let length = segment.data.len() as u32;
            if segment.address < start || segment.address + length > end || length == 0 {
                continue;
            }
            let offset = (segment.address - start) as usize;
            if image.len() < offset + length as usize {
                image.resize(offset + length as usize, 0);
            }
            image[offset..offset + length as usize]
                .copy_from_slice(&self.bytes[segment.data.clone()]);
        }
        image
    }

    // A NUL-terminated string at `address` in the loaded image.
    fn c_string(&self, address: u32) -> String {
        for segment in &self.segments {
            let length = segment.data.len() as u32;
            if address >= segment.address && address < segment.address + length {
                let start = segment.data.start + (address - segment.address) as usize;
                let text = &self.bytes[start..segment.data.end];
                let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
                return String::from_utf8_lossy(text).into_owned();
            }
        }
        format!("<string at {address:#010x}>")
    }

    // The function containing `address`, as `in NAME+OFFSET`.
    fn describe(&self, address: u32) -> String {
        let address = address & !1;
        let function = self
            .symbols
            .iter()
            .filter(|symbol| symbol.function)
            .filter(|symbol| {
                let start = symbol.value & !1;
                start <= address && (symbol.size == 0 || address < start + symbol.size)
            })
            .max_by_key(|symbol| symbol.value);
        match function {
            Some(symbol) => format!("in {}+{:#x}", symbol.name, address - (symbol.value & !1)),
            None if address < 0x1_0000 => "in the boot ROM".to_owned(),
            None => String::new(),
        }
    }
}
