//! Chip-independent firmware image handling: reading the loadable segments of
//! a linked ELF file, UF2 packing, and UF2 bootloader drives.
use std::path::{Path, PathBuf};

/// One file-backed PT_LOAD segment, by load (physical) address.
#[derive(Debug)]
pub struct Segment {
    /// Offset of the segment's bytes in the ELF file.
    pub offset: usize,
    pub address: u32,
    pub size: usize,
}

impl Segment {
    pub fn end(&self) -> u64 {
        self.address as u64 + self.size as u64
    }
}

/// The file-backed PT_LOAD segments of a little-endian ELF32 executable,
/// sorted by load address.
pub fn load_segments(elf: &[u8]) -> Result<Vec<Segment>, String> {
    let malformed = || "linker output is not a little-endian 32-bit ELF file".to_string();
    if elf.len() < 52 || &elf[..4] != b"\x7fELF" || elf[4] != 1 || elf[5] != 1 {
        return Err(malformed());
    }
    let u16_at = |at: usize| -> Result<usize, String> {
        elf.get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
            .ok_or_else(malformed)
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        elf.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(malformed)
    };
    let table = u32_at(0x1C)? as usize;
    let entry_size = u16_at(0x2A)?;
    let count = u16_at(0x2C)?;
    let mut segments = vec![];
    for index in 0..count {
        let header = table + index * entry_size;
        const PT_LOAD: u32 = 1;
        let size = u32_at(header + 16)? as usize;
        if u32_at(header)? != PT_LOAD || size == 0 {
            continue;
        }
        let offset = u32_at(header + 4)? as usize;
        if offset.checked_add(size).is_none_or(|end| end > elf.len()) {
            return Err(malformed());
        }
        segments.push(Segment {
            offset,
            address: u32_at(header + 12)?,
            size,
        });
    }
    segments.sort_by_key(|s| s.address);
    Ok(segments)
}

/// Pack segments into 256-byte UF2 blocks (https://github.com/microsoft/uf2),
/// padding partial pages with zeros. Blocks are sorted by address.
pub fn uf2(elf: &[u8], segments: &[Segment], family: u32) -> Vec<u8> {
    const PAGE: u32 = 256;
    let mut pages = std::collections::BTreeMap::<u32, [u8; PAGE as usize]>::new();
    for segment in segments {
        for (index, &byte) in elf[segment.offset..segment.offset + segment.size]
            .iter()
            .enumerate()
        {
            let address = segment.address + index as u32;
            let page = pages
                .entry(address & !(PAGE - 1))
                .or_insert([0; PAGE as usize]);
            page[(address % PAGE) as usize] = byte;
        }
    }
    let total = pages.len() as u32;
    let mut out = Vec::with_capacity(pages.len() * 512);
    for (number, (address, data)) in pages.iter().enumerate() {
        const FAMILY_ID_PRESENT: u32 = 0x0000_2000;
        for word in [
            0x0A32_4655,
            0x9E5D_5157,
            FAMILY_ID_PRESENT,
            *address,
            PAGE,
            number as u32,
            total,
            family,
        ] {
            out.extend_from_slice(&u32::to_le_bytes(word));
        }
        out.extend_from_slice(data);
        out.resize(out.len() + 476 - PAGE as usize, 0);
        out.extend_from_slice(&u32::to_le_bytes(0x0AB1_6F30));
    }
    out
}

/// Mounted UF2 bootloader drives whose INFO_UF2.TXT names `board_id`, such
/// as the RPI-RP2 volume an RP2040 shows while BOOTSEL is held at power-up.
pub fn uf2_drives(board_id: &str) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        roots.extend((b'D'..=b'Z').map(|letter| PathBuf::from(format!("{}:\\", letter as char))));
    } else {
        let mut parents = vec![PathBuf::from("/Volumes"), PathBuf::from("/media")];
        if let Some(user) = std::env::var_os("USER") {
            parents.push(Path::new("/media").join(&user));
            parents.push(Path::new("/run/media").join(&user));
        }
        for parent in parents {
            if let Ok(entries) = std::fs::read_dir(parent) {
                roots.extend(entries.flatten().map(|entry| entry.path()));
            }
        }
    }
    roots.sort();
    roots.dedup();
    let wanted = format!("Board-ID: {board_id}");
    roots
        .into_iter()
        .filter(|root| {
            std::fs::read_to_string(root.join("INFO_UF2.TXT"))
                .is_ok_and(|info| info.lines().any(|line| line.trim() == wanted))
        })
        .collect()
}

/// Copy a UF2 image to a bootloader drive. The board reboots into the new
/// firmware as soon as the last block arrives, which can make the final write
/// or close fail even though flashing succeeded; a vanished drive counts as
/// success.
pub fn write_uf2_drive(image: &[u8], drive: &Path) -> Result<(), String> {
    let target = drive.join("firmware.uf2");
    match std::fs::write(&target, image) {
        Ok(()) => Ok(()),
        Err(_) if !drive.join("INFO_UF2.TXT").exists() => Ok(()),
        Err(e) => Err(format!("cannot write {}: {e}", target.display())),
    }
}
