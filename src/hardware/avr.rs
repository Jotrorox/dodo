//! What the 8-bit AVR chips share: their runtime helpers, assembled from the
//! chip's startup code, `avr_builtins.ll`, the shared soft-float code with
//! native binary32 arithmetic (`avr_float32.ll`) and AVR names
//! (`avr_float.ll`); and Intel HEX, the image format their bootloaders and
//! tools take.
//!
//! AVR is a Harvard architecture. LLVM puts code in address space 1, and a
//! function value is a word address in it; data, including constants, is in
//! address space 0, the SRAM. The code generator keeps the two apart
//! (`code_space` in `src/codegen.rs`) and places panic messages in program
//! memory, where the chip runtime's panic hook expects them.
use std::borrow::Cow;

/// The runtime IR of an AVR chip: its startup code, which declares the data
/// layout, followed by the integer and floating-point helpers.
pub fn runtime(chip: &str) -> Cow<'static, str> {
    let mut text = String::from(chip);
    text.push_str(include_str!("avr_builtins.ll"));
    text.push_str(&soft_float(concat!(
        include_str!("arm_float.ll"),
        include_str!("arm_float_soft.ll")
    )));
    text.push_str(include_str!("avr_float32.ll"));
    text.push_str(include_str!("avr_float.ll"));
    Cow::Owned(text)
}

/// The GNU comparison helpers of the shared soft-float code, which return a
/// 32-bit int; `avr_float.ll` defines AVR's, which return 8 bits.
const WIDE_COMPARISONS: [&str; 4] = ["__lesf2", "__gesf2", "__ledf2", "__gedf2"];

/// The shared soft-float code without its aliases, which name functions in
/// the default address space, and with its 32-bit GNU comparisons renamed out
/// of the way. `avr_float.ll` gives the helpers their AVR names.
fn soft_float(shared: &str) -> String {
    let mut out = String::with_capacity(shared.len());
    for line in shared.lines() {
        if line.starts_with('@') && line.contains(" alias ") {
            continue;
        }
        let mut line = Cow::Borrowed(line);
        for name in WIDE_COMPARISONS {
            let symbol = format!("@{name}(");
            if line.contains(&symbol) {
                line = Cow::Owned(line.replace(&symbol, &format!("@dodo_arm{name}(")));
            }
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// `image` at `origin` as Intel HEX text: 16-byte data records, extended
/// linear address records above 64 KiB, and the end-of-file record. Runs of
/// erased flash (0xFF) at the end of a record are left out.
pub fn intel_hex(image: &[u8], origin: u32) -> String {
    fn record(out: &mut String, address: u16, kind: u8, data: &[u8]) {
        let mut sum = data.len() as u8;
        sum = sum.wrapping_add((address >> 8) as u8);
        sum = sum.wrapping_add(address as u8);
        sum = sum.wrapping_add(kind);
        out.push_str(&format!(":{:02X}{address:04X}{kind:02X}", data.len()));
        for &byte in data {
            out.push_str(&format!("{byte:02X}"));
            sum = sum.wrapping_add(byte);
        }
        out.push_str(&format!("{:02X}\n", sum.wrapping_neg()));
    }
    let mut out = String::new();
    let mut upper = 0u16;
    for (index, chunk) in image.chunks(16).enumerate() {
        let used = chunk.len() - chunk.iter().rev().take_while(|&&b| b == 0xFF).count();
        if used == 0 {
            continue;
        }
        let address = origin + (index * 16) as u32;
        if (address >> 16) as u16 != upper {
            upper = (address >> 16) as u16;
            record(&mut out, 0, 4, &upper.to_be_bytes());
        }
        record(&mut out, address as u16, 0, &chunk[..used]);
    }
    record(&mut out, 0, 1, &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_hex_records_have_checksums_and_skip_erased_bytes() {
        let mut image = vec![0x0C, 0x94, 0x34, 0x00];
        image.resize(16, 0xFF);
        image.extend_from_slice(&[0xFF; 16]);
        image.extend_from_slice(&[1, 2]);
        let text = intel_hex(&image, 0);
        assert_eq!(text, ":040000000C94340028\n:020020000102DB\n:00000001FF\n");
    }

    #[test]
    fn intel_hex_switches_the_upper_address_above_64_kib() {
        let text = intel_hex(&[0xAA], 0x1_0000);
        assert_eq!(text, ":020000040001F9\n:01000000AA55\n:00000001FF\n");
    }

    #[test]
    fn soft_float_drops_aliases_and_renames_wide_comparisons() {
        let shared = "define weak i32 @__lesf2(i32 %a, i32 %b) #1 {\n  %r = call i32 @cmp32(i32 %a, i32 %b, i32 1)\n}\n@__ltsf2 = weak alias i32 (i32, i32), ptr @__lesf2\n@__aeabi_dadd = weak alias i64 (i64, i64), ptr @dodo_soft_dadd\n";
        let text = soft_float(shared);
        assert!(text.contains("define weak i32 @dodo_arm__lesf2(i32 %a"));
        assert!(!text.contains(" alias "));
        assert!(text.contains("@cmp32("));
    }

    /// Every helper the runtime gives an AVR name is defined, and the
    /// shared code's names do not clash with AVR's.
    #[test]
    fn runtime_defines_the_gnu_names_once() {
        let text = runtime(include_str!("atmega328p/runtime.ll"));
        for name in [
            "__lesf2",
            "__gesf2",
            "__ledf2",
            "__gedf2",
            "__unordsf2",
            "__unorddf2",
        ] {
            let definitions = text.matches(&format!(" @{name}(")).count();
            assert_eq!(definitions, 1, "{name}");
        }
        for name in [
            "__addsf3",
            "__divdf3",
            "__fixsfsi",
            "__floatundidf",
            "__ltdf2",
        ] {
            assert!(text.contains(&format!("@{name} = weak alias")), "{name}");
        }
        assert!(text.starts_with("; Microchip ATmega328P"));
        assert!(text.contains("define weak cc 87 { i16, i16 } @__udivmodhi4"));
    }
}
