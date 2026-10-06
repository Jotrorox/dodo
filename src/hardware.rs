//! Firmware platforms: the microcontroller chips Dodo can build for, the
//! boards built around them, and turning a linked program into an image the
//! board's bootloader accepts.
//!
//! The split follows the hardware. A [`Chip`] holds everything fixed by the
//! silicon: instruction set, memory map, startup runtime (LLVM IR, compiled by
//! the compiler itself), linker script, image format, and the standard library
//! package with its peripheral support (`std/embedded/chip/NAME`). A [`Board`]
//! names a chip and adds what the circuit board decides: flash size and the
//! board package (`std/embedded/board/NAME`) that knows its crystal, LEDs, and
//! pin wiring.
//!
//! A build selects a platform with `board = "NAME"`, or `chip = "NAME"` for a
//! custom board. Programs then import `std/embedded/board` or
//! `std/embedded/chip`, which resolve to the selected packages, so application
//! code is not tied to one board.
//!
//! Adding a chip: write `src/hardware/CHIP.rs` with a `CHIP` descriptor, its
//! runtime and linker script, add the `std/embedded/chip/CHIP` package, and
//! list the chip in [`CHIPS`]. Adding a board: add a [`BOARDS`] entry and a
//! `std/embedded/board/BOARD` package that follows the board contract in
//! `std/embedded/hal`.
use std::path::PathBuf;

pub mod image;
mod rp2040;
mod rp2350;

#[derive(Debug)]
pub struct Chip {
    pub name: &'static str,
    pub description: &'static str,
    pub triple: &'static str,
    pub cpu: &'static str,
    /// Execute-in-place flash base address.
    pub flash_origin: u32,
    /// Flash size assumed with `chip = NAME` and no board.
    pub default_flash_size: u32,
    pub ram: Region,
    /// Startup code and runtime helpers, as LLVM IR text.
    pub runtime: &'static str,
    /// Linker script sections; the platform prepends its MEMORY block.
    pub linker_script: &'static str,
    /// The C symbol the runtime defines weakly as the panic hook.
    pub panic_hook: &'static str,
    /// The default ELF linker.
    pub linker: &'static str,
    /// The standard library package `import "std/embedded/chip"` selects.
    pub package: &'static str,
    pub image: ImageFormat,
    /// Chip-specific changes to the linked ELF, such as boot checksums.
    pub fixup: Fixup,
}

/// Changes a chip needs in its linked ELF file, given its loadable segments.
pub type Fixup = fn(&Platform, &mut [u8], &[image::Segment]) -> Result<(), String>;

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub origin: u32,
    pub length: u32,
}

/// The flashable image written next to the ELF output.
#[derive(Clone, Copy, Debug)]
pub enum ImageFormat {
    /// USB Flashing Format, with the bootloader drive's INFO_UF2.TXT Board-ID.
    Uf2 { family: u32, drive_id: &'static str },
}

#[derive(Debug)]
pub struct Board {
    pub name: &'static str,
    pub description: &'static str,
    pub chip: &'static Chip,
    pub flash_size: u32,
    /// The standard library package `import "std/embedded/board"` selects.
    pub package: &'static str,
}

pub static CHIPS: &[&Chip] = &[&rp2040::CHIP, &rp2350::CHIP];

pub static BOARDS: &[Board] = &[
    Board {
        name: "pico",
        description: "Raspberry Pi Pico",
        chip: &rp2040::CHIP,
        flash_size: 2 * 1024 * 1024,
        package: "std/embedded/board/pico",
    },
    Board {
        name: "pico2",
        description: "Raspberry Pi Pico 2",
        chip: &rp2350::CHIP,
        flash_size: 4 * 1024 * 1024,
        package: "std/embedded/board/pico2",
    },
];

pub fn chip(name: &str) -> Option<&'static Chip> {
    CHIPS.iter().copied().find(|chip| chip.name == name)
}

pub fn board(name: &str) -> Option<&'static Board> {
    BOARDS.iter().find(|board| board.name == name)
}

pub fn unknown_board(name: &str) -> String {
    unknown("board", name, BOARDS.iter().map(|b| b.name))
}

pub fn unknown_chip(name: &str) -> String {
    unknown("chip", name, CHIPS.iter().map(|c| c.name))
}

fn unknown<'a>(kind: &str, name: &str, choices: impl Iterator<Item = &'a str> + Clone) -> String {
    format!(
        "unknown {kind} '{name}'{}; available {kind}s: {}",
        crate::project::suggest(name, choices.clone()),
        choices.collect::<Vec<_>>().join(", ")
    )
}

/// The hardware a firmware build targets: a chip, and the board when known.
#[derive(Clone, Copy, Debug)]
pub struct Platform {
    pub chip: &'static Chip,
    pub board: Option<&'static Board>,
}

impl Platform {
    /// Resolve `board` and `chip` settings. A board implies its chip; naming
    /// both requires them to agree.
    pub fn select(board: Option<&str>, chip: Option<&str>) -> Result<Option<Self>, String> {
        let board = board
            .map(|name| self::board(name).ok_or_else(|| unknown_board(name)))
            .transpose()?;
        let chip = chip
            .map(|name| self::chip(name).ok_or_else(|| unknown_chip(name)))
            .transpose()?;
        Ok(match (board, chip) {
            (Some(board), Some(chip)) if chip.name != board.chip.name => {
                return Err(format!(
                    "board '{}' uses chip '{}', not '{}'",
                    board.name, board.chip.name, chip.name
                ));
            }
            (Some(board), _) => Some(Self {
                chip: board.chip,
                board: Some(board),
            }),
            (None, Some(chip)) => Some(Self { chip, board: None }),
            (None, None) => None,
        })
    }

    /// The board name, or the chip name for a custom board.
    pub fn name(&self) -> &'static str {
        self.board.map_or(self.chip.name, |board| board.name)
    }

    pub fn description(&self) -> &'static str {
        self.board
            .map_or(self.chip.description, |board| board.description)
    }

    pub fn flash(&self) -> Region {
        Region {
            origin: self.chip.flash_origin,
            length: self
                .board
                .map_or(self.chip.default_flash_size, |board| board.flash_size),
        }
    }

    pub fn linker_script(&self) -> String {
        let flash = self.flash();
        let ram = self.chip.ram;
        format!(
            "MEMORY\n{{\n    FLASH (rx) : ORIGIN = {:#010x}, LENGTH = {:#x}\n    RAM (rwx) : ORIGIN = {:#010x}, LENGTH = {:#x}\n}}\n\n{}",
            flash.origin, flash.length, ram.origin, ram.length, self.chip.linker_script
        )
    }

    /// The extension of the flashable image written next to the ELF.
    pub fn image_extension(&self) -> &'static str {
        match self.chip.image {
            ImageFormat::Uf2 { .. } => "uf2",
        }
    }

    /// Check the linked ELF against flash, apply chip fixups in place, and
    /// return the flashable image.
    pub fn finish(&self, elf: &mut [u8]) -> Result<Vec<u8>, String> {
        let segments = image::load_segments(elf)?;
        let flash = self.flash();
        for segment in &segments {
            if segment.address < flash.origin
                || segment.end() > flash.origin as u64 + flash.length as u64
            {
                return Err(format!(
                    "firmware segment at {:#010x} ({} bytes) is outside {}'s {} KiB flash",
                    segment.address,
                    segment.size,
                    self.name(),
                    flash.length / 1024
                ));
            }
        }
        (self.chip.fixup)(self, elf, &segments)?;
        Ok(match self.chip.image {
            ImageFormat::Uf2 { family, .. } => image::uf2(elf, &segments, family),
        })
    }

    /// Mounted bootloader drives that accept this platform's images.
    pub fn bootloader_drives(&self) -> Vec<PathBuf> {
        match self.chip.image {
            ImageFormat::Uf2 { drive_id, .. } => image::uf2_drives(drive_id),
        }
    }

    pub fn bootloader_drive_name(&self) -> &'static str {
        match self.chip.image {
            ImageFormat::Uf2 { drive_id, .. } => drive_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elf(segments: &[(u32, &[u8])]) -> Vec<u8> {
        let mut file = vec![0u8; 52];
        file[..6].copy_from_slice(b"\x7fELF\x01\x01");
        file[0x1C..0x20].copy_from_slice(&52u32.to_le_bytes());
        file[0x2A..0x2C].copy_from_slice(&32u16.to_le_bytes());
        file[0x2C..0x2E].copy_from_slice(&(segments.len() as u16).to_le_bytes());
        let mut data_offset = 52 + 32 * segments.len();
        let mut data = vec![];
        for (address, bytes) in segments {
            for word in [
                1,
                data_offset as u32,
                *address,
                *address,
                bytes.len() as u32,
                bytes.len() as u32,
                5,
                4,
            ] {
                file.extend_from_slice(&word.to_le_bytes());
            }
            data.extend_from_slice(bytes);
            data_offset += bytes.len();
        }
        file.extend(data);
        file
    }

    fn pico() -> Platform {
        Platform::select(Some("pico"), None).unwrap().unwrap()
    }

    #[test]
    fn boards_imply_their_chip_and_reject_another() {
        let platform = pico();
        assert_eq!(platform.chip.name, "rp2040");
        assert_eq!(platform.name(), "pico");
        let custom = Platform::select(None, Some("rp2040")).unwrap().unwrap();
        assert!(custom.board.is_none());
        assert_eq!(custom.name(), "rp2040");
        assert!(Platform::select(None, None).unwrap().is_none());
        assert!(
            Platform::select(Some("pcio"), None)
                .unwrap_err()
                .contains("unknown board 'pcio'; did you mean 'pico'?")
        );
        assert!(
            Platform::select(None, Some("rp2041"))
                .unwrap_err()
                .contains("available chips: rp2040, rp2350")
        );
    }

    #[test]
    fn every_board_uses_a_listed_chip_and_names_its_packages() {
        for board in BOARDS {
            assert!(CHIPS.iter().any(|chip| std::ptr::eq(*chip, board.chip)));
            assert_eq!(board.package, format!("std/embedded/board/{}", board.name));
        }
        for chip in CHIPS {
            assert_eq!(chip.package, format!("std/embedded/chip/{}", chip.name));
        }
    }

    #[test]
    fn finish_patches_boot2_crc_and_packs_uf2_blocks() {
        let boot = [0xA5u8; 256];
        let tail = [1u8, 2, 3];
        let mut file = elf(&[(0x1000_0000, &boot), (0x1000_0300, &tail)]);
        let image = pico().finish(&mut file).unwrap();
        let crc = rp2040::crc32_mpeg2(&[0xA5; 252]);
        let patched = &file[52 + 64 + 252..52 + 64 + 256];
        assert_eq!(patched, crc.to_le_bytes());

        assert_eq!(image.len(), 2 * 512);
        let word = |block: usize, index: usize| {
            let at = block * 512 + index * 4;
            u32::from_le_bytes(image[at..at + 4].try_into().unwrap())
        };
        assert_eq!(word(0, 0), 0x0A32_4655);
        assert_eq!(word(0, 3), 0x1000_0000);
        assert_eq!(word(0, 6), 2);
        assert_eq!(word(0, 7), 0xE48B_FF56);
        assert_eq!(&image[32 + 252..32 + 256], crc.to_le_bytes());
        assert_eq!(word(1, 3), 0x1000_0300);
        assert_eq!(word(1, 5), 1);
        assert_eq!(&image[512 + 32..512 + 35], &tail);
        assert_eq!(word(1, 127), 0x0AB1_6F30);
    }

    #[test]
    fn rp2350_images_need_an_image_def_and_use_the_arm_secure_family() {
        let platform = Platform::select(Some("pico2"), None).unwrap().unwrap();
        assert_eq!(platform.chip.name, "rp2350");
        assert_eq!(platform.bootloader_drive_name(), "RP2350");
        let mut block = [0u8; 0x124];
        block[0x110..0x114].copy_from_slice(&0xFFFF_DED3u32.to_le_bytes());
        let mut file = elf(&[(0x1000_0000, &block)]);
        let image = platform.finish(&mut file).unwrap();
        assert_eq!(image.len(), 2 * 512);
        assert_eq!(
            u32::from_le_bytes(image[28..32].try_into().unwrap()),
            0xE48B_FF59
        );

        let mut file = elf(&[(0x1000_0000, &[0; 0x124])]);
        assert!(
            platform
                .finish(&mut file)
                .unwrap_err()
                .contains("IMAGE_DEF")
        );
        let script = platform.linker_script();
        assert!(script.contains("FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 0x400000"));
        assert!(script.contains("RAM (rwx) : ORIGIN = 0x20000000, LENGTH = 0x82000"));
    }

    #[test]
    fn finish_rejects_images_outside_flash() {
        let mut file = elf(&[(0x1000_0000, &[0; 256]), (0x2000_0000, &[1])]);
        assert!(pico().finish(&mut file).unwrap_err().contains("outside"));
    }

    #[test]
    fn linker_script_declares_board_memory() {
        let script = pico().linker_script();
        assert!(script.contains("FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 0x200000"));
        assert!(script.contains("RAM (rwx) : ORIGIN = 0x20000000, LENGTH = 0x42000"));
    }
}
