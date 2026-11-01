//! Inline assembly, `@interrupt` handlers, and `@section` / `@align`
//! placement: execution on the host, emitted code for firmware targets, linked
//! RP2040 and RP2350 images (when ld.lld is installed), and diagnostics.
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

fn success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-firmware-attributes-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_owned()
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

/// Build `source` with `args` and return the error output; it must fail.
fn rejected(scratch: &Scratch, source: &str, args: &[&str]) -> String {
    let file = scratch.write("rejected.dodo", source);
    let output_path = scratch.path("rejected.out");
    let mut all = vec!["build", file.as_str(), "-o", output_path.as_str()];
    all.extend_from_slice(args);
    let output = dodo(&all);
    assert!(!output.status.success(), "accepted:\n{source}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn expect_rejected(scratch: &Scratch, source: &str, args: &[&str], message: &str) {
    let error = rejected(scratch, source, args);
    assert!(
        error.contains(message),
        "expected {message:?} for\n{source}\ngot\n{error}"
    );
}

/// Emit `kind` (asm, llvm-ir, obj) for `source` and return the text.
fn emit(scratch: &Scratch, source: &str, kind: &str, args: &[&str]) -> String {
    let file = scratch.write("emit.dodo", source);
    let output_path = scratch.path(&format!("emit.{kind}"));
    let mut all = vec![
        "build",
        file.as_str(),
        "--emit",
        kind,
        "-o",
        output_path.as_str(),
    ];
    all.extend_from_slice(args);
    success(&dodo(&all));
    fs::read_to_string(&output_path).unwrap_or_default()
}

/// Templates for the host: adds, moves into a fixed register, and a no-op.
fn host_templates() -> Option<[&'static str; 4]> {
    match std::env::consts::ARCH {
        "aarch64" => Some([
            "add {0}, {1}, {2}",
            "add {0}, {0}, #1",
            "mov {0}, #7\nmov {1}, #9",
            "x9",
        ]),
        "x86_64" => Some([
            "lea ({1},{2}), {0}",
            "inc {0}",
            "mov $$7, {0}\nmov $$9, {1}",
            "rcx",
        ]),
        _ => None,
    }
}

#[test]
fn inline_assembly_runs_on_the_host() {
    let Some([add, increment, pair, register]) = host_templates() else {
        eprintln!("skipping: no inline assembly fixture for this host");
        return;
    };
    // Dodo strings cannot hold a raw newline from Rust formatting directly.
    let pair = pair.replace('\n', "\", \"").replace("$$", "$");
    let source = format!(
        r#"package main
import "std/console"

fn add(a: u64, b: u64) -> u64 {{
    return unsafe {{ asm("{add}", out(reg) u64, in(reg) a, in(reg) b) }}
}}

// Generic outputs follow the instantiated type.
fn through<T>(value: T) -> T {{
    return unsafe {{ asm("", inout(reg) value) }}
}}

fn main() {{
    console.printf("{{}}\n", add(40, 2))!
    counter: u64 = 5
    counter = unsafe {{ asm("{increment}", inout(reg) counter) }}
    console.printf("{{}}\n", counter)!
    let (seven, nine) = unsafe {{ asm("{pair}", out(reg) u64, out("{register}") u64) }}
    console.printf("{{}} {{}}\n", seven, nine)!
    console.printf("{{}} {{}}\n", through(3u8), through(-4i32))!
    unsafe {{
        asm("")
        cached := asm("{add}", out(reg) u64, in(reg) 1u64, in(reg) 2u64, options(pure, nomem))
        console.printf("{{}}\n", cached)!
    }}
}}
"#
    );
    let scratch = Scratch::new("host");
    let file = scratch.write("asm.dodo", &source);
    for level in ["0", "2"] {
        let executable = scratch.path(&format!("asm-O{level}{}", std::env::consts::EXE_SUFFIX));
        success(&dodo(&["build", &file, "-O", level, "-o", &executable]));
        let output = success(&Command::new(&executable).output().unwrap());
        assert_eq!(output, "42\n6\n7 9\n3 -4\n3\n", "-O{level}");
    }
}

#[test]
fn inline_assembly_mistakes_are_diagnosed() {
    let scratch = Scratch::new("asm-errors");
    let cases: &[(&str, &str)] = &[
        (
            "fn main() { asm(\"nop\") }",
            "inline assembly requires an explicit unsafe block",
        ),
        (
            "fn main() { unsafe { asm(\"mov {1}, {0}\", in(reg) 1u32) } }",
            "refers to operand 1, but the asm has 1 operand",
        ),
        (
            "fn main() { unsafe { asm(\"{\") } }",
            "unclosed `{` in assembly template",
        ),
        (
            "fn main() { unsafe { asm(\"{0:}\", in(reg) 1u32) } }",
            "invalid operand modifier",
        ),
        (
            "fn main() { unsafe { value := asm(\"\", out(reg) f64) } }",
            "asm operands must be integers",
        ),
        (
            "fn main() { unsafe { value := asm(\"\", out(reg) u128) } }",
            "asm operands must be integers of at most",
        ),
        (
            "fn main() { unsafe { asm(\"nop\", options(pure)) } }",
            "options(pure) asm must have an output",
        ),
        (
            "fn main() { unsafe { asm(\"\", in(\"r1\") 1u32, in(\"R1\") 2u32) } }",
            "is used by two asm operands",
        ),
        (
            "fn main() { unsafe { value := asm(\"\", out(\"r1\") u32, clobber(\"r1\")) } }",
            "is both an asm output and a clobber",
        ),
        (
            "fn main() { unsafe { asm(\"nop\", options(fast)) } }",
            "unknown asm option",
        ),
        (
            "fn main() { unsafe { asm(b\"nop\") } }",
            "not a byte string",
        ),
        (
            "fn main() { unsafe { asm(\"nop\", in(r0) 1u32) } }",
            "expected `reg` or a register name",
        ),
        ("fn main() { asm := 1 }", "expected"),
    ];
    for (body, message) in cases {
        expect_rejected(&scratch, &format!("package main\n{body}\n"), &[], message);
    }
    // The assembler's rejection is a compile error, not a crash.
    let error = rejected(
        &scratch,
        "package main\nfn main() { unsafe { asm(\"not_an_instruction r0\") } }\n",
        &["--target", "thumbv6m-none-eabi", "--emit", "obj"],
    );
    assert!(error.contains("not_an_instruction"), "{error}");
}

#[test]
fn firmware_uses_inline_assembly_and_the_cortex_m_package() {
    let scratch = Scratch::new("cortex-m");
    let assembly = emit(
        &scratch,
        r#"package main
import "std/embedded/cortex_m"

pub fn swap_halves(value: u32) -> (u32, u32) {
    return unsafe {
        asm(
            "lsrs {0}, {2}, #16",
            "uxth {1}, {2}",
            out(reg) u32,
            out(reg) u32,
            in(reg) value,
        )
    }
}

pub fn control() -> u32 {
    cortex_m.enable(14)
    cortex_m.set_priority(14, 0x40)
    cortex_m.data_barrier()
    unsafe {
        cortex_m.enable_interrupts()
        asm("svc #1", in("r0") 5u32, clobber("r1"))
    }
    return cortex_m.active_exception() + cortex_m.priority(14) as u32
}
"#,
        "asm",
        &[
            "--target",
            "thumbv6m-none-eabi",
            "--cpu",
            "cortex-m0plus",
            "-O",
            "2",
        ],
    );
    // LLVM prints most instructions with a tab after the mnemonic.
    let assembly = assembly.replace('\t', " ");
    for instruction in ["lsrs", "uxth", "cpsie i", "dsb", "svc #1", "mrs", "ipsr"] {
        assert!(
            assembly.contains(instruction),
            "missing {instruction}:\n{assembly}"
        );
    }
    // The interrupt-driven example builds for every board.
    let example = root().join("examples/interrupts");
    for board in ["pico", "pico2", "pico2_w"] {
        let object = scratch.path(&format!("interrupts-{board}.o"));
        success(&dodo(&[
            "build",
            example.to_str().unwrap(),
            "--board",
            board,
            "--emit",
            "obj",
            "-o",
            &object,
        ]));
    }
}

#[test]
fn riscv_interrupt_handlers_return_with_mret() {
    let scratch = Scratch::new("riscv");
    let assembly = emit(
        &scratch,
        "package main\nstatic mut COUNT: u32 = 0\n@interrupt(MachineTimer)\nfn tick() {\n    unsafe {\n        COUNT += 1\n    }\n}\n",
        "asm",
        &["--target", "riscv32-unknown-none-elf", "-O", "2"],
    );
    let handler = assembly
        .split("MachineTimer:")
        .nth(1)
        .expect("exported handler");
    let body = handler.split(".Lfunc_end").next().unwrap();
    assert!(assembly.contains(".globl\tMachineTimer"), "{assembly}");
    assert!(body.contains("mret") && !body.contains("\tret"), "{body}");
}

#[test]
fn interrupt_handler_mistakes_are_diagnosed() {
    let scratch = Scratch::new("interrupt-errors");
    let pico = ["--board", "pico", "--emit", "obj"];
    expect_rejected(
        &scratch,
        "package main\n@interrupt(TIMER_IRQ0)\nfn alarm() {}\nfn main() {}\n",
        &pico,
        "unknown interrupt vector `TIMER_IRQ0` for this chip; did you mean 'TIMER_IRQ_0'?",
    );
    // RP2350 names its timers differently.
    expect_rejected(
        &scratch,
        "package main\n@interrupt(TIMER_IRQ_0)\nfn alarm() {}\nfn main() {}\n",
        &["--board", "pico2", "--emit", "obj"],
        "TIMER0_IRQ_0, TIMER0_IRQ_1",
    );
    let cases: &[(&str, &str)] = &[
        (
            "@interrupt(TIMER_IRQ_0)\nfn alarm() {}\nfn main() { alarm() }",
            "`alarm` is the TIMER_IRQ_0 interrupt handler; only the hardware calls it",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\nfn alarm() {}\nfn main() { handler: fn() = alarm }",
            "only the hardware calls it",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\nfn a() {}\n@interrupt(TIMER_IRQ_0)\nfn b() {}\nfn main() {}",
            "`TIMER_IRQ_0` already has a handler",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\nfn a() {}\nextern \"C\" fn TIMER_IRQ_0() {}\nfn main() {}",
            "uses the symbol of the @interrupt(TIMER_IRQ_0) handler",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\nfn a(level: u32) {}\nfn main() {}",
            "@interrupt requires a private, safe, non-generic fn name()",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\nfn a() -> u32 { return 1 }\nfn main() {}",
            "@interrupt requires",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\npub fn a() {}\nfn main() {}",
            "@interrupt requires",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\nunsafe fn a() {}\nfn main() {}",
            "@interrupt requires",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\nfn a<T>() {}\nfn main() {}",
            "@interrupt requires",
        ),
        (
            "@interrupt(TIMER_IRQ_0)\nstatic X: u32 = 1\nfn main() {}",
            "@interrupt applies only to functions",
        ),
        (
            "struct S {\n    @interrupt(TIMER_IRQ_0)\n    fn tick() {}\n}\nfn main() {}",
            "@interrupt applies only to top-level functions",
        ),
        (
            "@interrupt(\"TIMER_IRQ_0\")\nfn a() {}\nfn main() {}",
            "@interrupt expects a vector name",
        ),
        (
            "@interrupt(A)\n@interrupt(B)\nfn a() {}\nfn main() {}",
            "duplicate @interrupt attribute",
        ),
    ];
    for (body, message) in cases {
        expect_rejected(&scratch, &format!("package main\n{body}\n"), &pico, message);
    }
}

#[test]
fn placement_mistakes_are_diagnosed() {
    let scratch = Scratch::new("placement-errors");
    let cases: &[(&str, &str)] = &[
        (
            "@section(\".x\")\nconst A: u32 = 1",
            "@section and @align apply only to statics; a `const` has no storage",
        ),
        (
            "@align(16)\nstruct S { x: u32 }",
            "@section and @align apply only to functions and statics",
        ),
        (
            "@align(3)\nstatic A: u32 = 1",
            "alignment must be a power of two",
        ),
        (
            "@align(0)\nstatic A: u32 = 1",
            "alignment must be a power of two",
        ),
        (
            "@align(1073741824)\nstatic A: u32 = 1",
            "alignment must be a power of two from 1 to 536870912 bytes",
        ),
        (
            "@section(\"\")\nstatic A: u32 = 1",
            "a section name must be nonempty",
        ),
        (
            "@section(\"has space\")\nstatic A: u32 = 1",
            "a section name must be nonempty",
        ),
        (
            "@section(4)\nstatic A: u32 = 1",
            "@section expects a string",
        ),
        (
            "@align(\"8\")\nstatic A: u32 = 1",
            "@align expects an integer",
        ),
        (
            "@section(\".a\")\n@section(\".b\")\nstatic A: u32 = 1",
            "duplicate @section attribute",
        ),
        (
            "interface I {\n    @align(8)\n    fn f(&self)\n}",
            "interface methods cannot have @interrupt, @section, or @align",
        ),
    ];
    for (declaration, message) in cases {
        expect_rejected(
            &scratch,
            &format!("package main\n{declaration}\nfn main() {{}}\n"),
            &["--target", "thumbv6m-none-eabi", "--emit", "obj"],
            message,
        );
    }
    // Mach-O sections need a segment.
    expect_rejected(
        &scratch,
        "package main\n@section(\".data.extra\")\nstatic A: u32 = 1\nfn main() {}\n",
        &["--target", "aarch64-apple-macos", "--emit", "obj"],
        "needs a Mach-O segment",
    );
}

const PLACED: &str = r#"package main
import "std/embedded/board"

static mut TICKS: u32 = 0

// Nothing refers to these, yet @section keeps them.
@section(".rodata.signature")
static SIGNATURE: u32 = 0xC0FFEE42
@section(".noinit")
static mut RESETS: u32 = 0

@align(256)
static mut VECTORS: [48]u32 = [0u32; 48]

@section(".ramfunc")
fn scale(value: u32) -> u32 {
    return core.wrapping_mul(value, 3)
}

@align(64)
fn aligned() -> u32 {
    return 7
}

@interrupt(SysTick_Handler)
fn tick() {
    unsafe {
        TICKS += 1
    }
}

@interrupt(UART0_IRQ)
fn serial() {
    unsafe {
        TICKS += 2
    }
}

fn main() {
    b := board.take()!
    unsafe {
        RESETS += 1
        VECTORS[0] = scale(TICKS) + aligned()
    }
    for {
        b.led.toggle()
        b.timer.delay_ms(100)
    }
}
"#;

#[test]
fn sections_and_alignment_place_code_and_data() {
    let scratch = Scratch::new("placement");
    // The object file records each choice, on any target.
    let ir = emit(&scratch, PLACED, "llvm-ir", &["--board", "pico"]);
    for text in [
        "section \".rodata.signature\"",
        "section \".noinit\"",
        "section \".ramfunc\"",
        "align 256",
        "align 64",
        "@llvm.used",
    ] {
        assert!(ir.contains(text), "missing {text} in\n{ir}");
    }
    if !ld_lld() {
        return;
    }
    for (board, irq) in [("pico", 20), ("pico2", 33)] {
        let elf_path = scratch.path(&format!("placed-{board}.elf"));
        let file = scratch.write("placed.dodo", PLACED);
        let output = dodo(&["build", &file, "--board", board, "-o", &elf_path]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let elf = fs::read(&elf_path).unwrap();
        let symbol = |name: &str| {
            elf_symbol(&elf, name).unwrap_or_else(|| panic!("{board}: no symbol {name}"))
        };
        // Runs from SRAM, copied there with .data.
        let scale = symbol("dodo.main.scale");
        assert!((0x2000_0000..0x2008_2000).contains(&scale), "{scale:#x}");
        assert_eq!(symbol("dodo.main.VECTORS") % 256, 0);
        assert_eq!(symbol("dodo.main.aligned") & !1 & 63, 0);
        assert!(symbol("dodo.main.SIGNATURE") < 0x2000_0000);
        // .noinit is in SRAM but no loadable segment holds it.
        let resets = symbol("dodo.main.RESETS");
        assert!(resets >= 0x2000_0000);
        assert!(!loaded(&elf, resets), "{board}: .noinit is loaded");
        // The vector table names the handlers: SysTick is entry 15, and the
        // UART0 interrupt entry 16 + its IRQ number.
        let table = symbol("__vector_table");
        assert_eq!(
            word_at(&elf, table + 15 * 4),
            Some(symbol("SysTick_Handler"))
        );
        assert_eq!(
            word_at(&elf, table + (16 + irq) * 4),
            Some(symbol("UART0_IRQ"))
        );
        assert_eq!(symbol("SysTick_Handler") & 1, 1, "Thumb handler");
    }
}

#[test]
fn constants_holding_addresses_stay_out_of_hosted_text_segments() {
    let scratch = Scratch::new("relro");
    let source = "package main\n\nconst NAME: &str = \"lua\"\nconst LIMIT: i64 = 7\n\n\
                  fn main() -> i32 {\n    return (NAME.len as i32) + (LIMIT as i32)\n}\n";
    // Position-independent code makes NAME's section writable; on hosted
    // ELF it belongs in RELRO, or GNU ld merges it into an RWX segment.
    let hosted = emit(
        &scratch,
        source,
        "asm",
        &["--target", "x86_64-unknown-linux-gnu"],
    );
    assert!(
        hosted.contains(".section\t.data.rel.ro.dodo.main.NAME,"),
        "{hosted}"
    );
    assert!(
        hosted.contains(".section\t.rodata.dodo.main.LIMIT,"),
        "{hosted}"
    );
    // Firmware linker scripts keep every constant in flash under .rodata.
    let firmware = emit(&scratch, source, "asm", &["--target", "thumbv6m-none-eabi"]);
    assert!(
        firmware.contains(".section\t.rodata.dodo.main.NAME,"),
        "{firmware}"
    );
}

#[test]
fn interrupt_handlers_fill_the_vector_table() {
    let scratch = Scratch::new("vectors");
    // Exported under the vector name with the C ABI, whatever the function is called.
    let assembly = emit(
        &scratch,
        "package main\nstatic mut N: u32 = 0\n@interrupt(TIMER_IRQ_0)\nfn alarm_fired() {\n    unsafe {\n        N += 1\n    }\n}\nfn main() {}\n",
        "asm",
        &["--board", "pico"],
    );
    assert!(assembly.contains(".globl\tTIMER_IRQ_0"), "{assembly}");
    assert!(!assembly.contains("alarm_fired"), "{assembly}");
    if !ld_lld() {
        return;
    }
    let example = root().join("examples/interrupts");
    for (board, irq) in [("pico", 20), ("pico2", 33), ("pico2_w", 33)] {
        let elf_path = scratch.path(&format!("interrupts-{board}.elf"));
        let output = dodo(&[
            "build",
            example.to_str().unwrap(),
            "--board",
            board,
            "-o",
            &elf_path,
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let elf = fs::read(&elf_path).unwrap();
        let table = elf_symbol(&elf, "__vector_table").unwrap();
        let handler = elf_symbol(&elf, "UART0_IRQ").unwrap();
        let default = elf_symbol(&elf, "DefaultHandler").unwrap();
        assert_ne!(handler, default);
        assert_eq!(
            word_at(&elf, table + (16 + irq) * 4),
            Some(handler),
            "{board}"
        );
        assert_eq!(
            word_at(&elf, table + 15 * 4),
            elf_symbol(&elf, "SysTick_Handler"),
            "{board}"
        );
        // Vectors without a handler keep the runtime's default.
        assert_eq!(word_at(&elf, table + 16 * 4), Some(default), "{board}");
        assert!(Path::new(&elf_path).with_extension("uf2").exists());
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

fn half(bytes: &[u8], at: usize) -> usize {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize
}

/// The value of the ELF32 symbol `name`, if the file defines it.
fn elf_symbol(elf: &[u8], name: &str) -> Option<u32> {
    let header = |index: usize| word(elf, 0x20) as usize + index * half(elf, 0x2E);
    let symbols = (0..half(elf, 0x30))
        .map(header)
        .find(|&at| word(elf, at + 4) == 2)?;
    let names = word(elf, header(word(elf, symbols + 24) as usize) + 16) as usize;
    let start = word(elf, symbols + 16) as usize;
    let end = start + word(elf, symbols + 20) as usize;
    (start..end).step_by(16).find_map(|at| {
        let offset = names + word(elf, at) as usize;
        let length = elf[offset..].iter().position(|&b| b == 0)?;
        let defined = half(elf, at + 14) != 0;
        (defined && &elf[offset..offset + length] == name.as_bytes()).then(|| word(elf, at + 4))
    })
}

/// File-backed PT_LOAD segments: (virtual address, file offset, file size).
fn segments(elf: &[u8]) -> Vec<(u32, usize, usize)> {
    let table = word(elf, 0x1C) as usize;
    (0..half(elf, 0x2C))
        .map(|index| table + index * half(elf, 0x2A))
        .filter(|&at| word(elf, at) == 1 && word(elf, at + 16) != 0)
        .map(|at| {
            (
                word(elf, at + 8),
                word(elf, at + 4) as usize,
                word(elf, at + 16) as usize,
            )
        })
        .collect()
}

fn loaded(elf: &[u8], address: u32) -> bool {
    segments(elf)
        .iter()
        .any(|&(start, _, size)| address >= start && ((address - start) as usize) < size)
}

/// The word at a loaded virtual address.
fn word_at(elf: &[u8], address: u32) -> Option<u32> {
    segments(elf).iter().find_map(|&(start, offset, size)| {
        let delta = address.checked_sub(start)? as usize;
        (delta + 4 <= size).then(|| word(elf, offset + delta))
    })
}
