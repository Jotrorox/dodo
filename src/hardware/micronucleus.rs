//! Flashing ATtiny boards through micronucleus, the USB bootloader of the
//! Digispark and its clones. The bootloader enumerates as a low-speed USB
//! device (16D0:0753) for a few seconds after the board is plugged in or
//! reset, takes vendor control requests, and then starts the program.
//!
//! A flash run waits for the bootloader, asks for its geometry, erases the
//! application flash, writes each page that holds part of the image, and
//! tells the bootloader to start the program. The bootloader cannot read
//! flash back, so there is no verification step.
//!
//! The bootloader keeps its own reset vector at address 0 and starts the
//! program through a jump it stores just below itself, the "tiny vector
//! table". Version 1 builds that table on the chip from the program's first
//! vectors, which must be `rjmp`s; from version 2 the host does, which
//! [`page_image`] mirrors after the micronucleus command-line tool.
use super::usb::{self, Device};
use std::time::{Duration, Instant};

pub const VENDOR: u16 = 0x16D0;
pub const PRODUCT: u16 = 0x0753;

const GET_INFO: u8 = 0;
const WRITE_PAGE: u8 = 1;
const ERASE: u8 = 2;
const WRITE_WORDS: u8 = 3;
const RUN: u8 = 4;

const TIMEOUT: Duration = Duration::from_millis(1000);
/// For asking a device that may be a running program instead.
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// What the bootloader reports about itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub major: u8,
    pub minor: u8,
    /// The flash the program may use, in bytes.
    pub flash_size: u16,
    pub page_size: u8,
    /// Where the bootloader starts, a page boundary.
    pub bootloader_start: u16,
    pub write_sleep: Duration,
    pub erase_sleep: Duration,
    /// The second and third signature bytes, from version 2 on.
    pub signature: Option<[u8; 2]>,
}

impl Info {
    /// Decode the GET_INFO answer of a bootloader version `release` (its
    /// bcdDevice).
    pub fn parse(release: u16, reply: &[u8]) -> Result<Info, String> {
        let (major, minor) = ((release >> 8) as u8, release as u8);
        let needed = if major >= 2 { 6 } else { 4 };
        if reply.len() < needed {
            return Err(format!(
                "the micronucleus {major}.{minor:02} bootloader answered {} bytes of information, not {needed}",
                reply.len()
            ));
        }
        let flash_size = u16::from_be_bytes([reply[0], reply[1]]);
        let page_size = reply[2];
        if page_size == 0 || flash_size == 0 {
            return Err("the micronucleus bootloader reports no flash".into());
        }
        let pages = (flash_size as u32).div_ceil(page_size as u32);
        let write_ms = (reply[3] & 0x7F) as u32;
        // Bit 7 marks chips that erase four pages at once (ATtiny441/841).
        let erase_ms = if major >= 2 && reply[3] & 0x80 != 0 {
            write_ms * pages / 4
        } else {
            write_ms * pages
        };
        Ok(Info {
            major,
            minor,
            flash_size,
            page_size,
            bootloader_start: (pages * page_size as u32) as u16,
            write_sleep: Duration::from_millis(write_ms as u64),
            erase_sleep: Duration::from_millis(erase_ms as u64),
            signature: (major >= 2).then(|| [reply[4], reply[5]]),
        })
    }
}

/// The pages to write, as (address, bytes), for `image` at address 0:
/// every page that holds part of it, padded with erased bytes, and the last
/// page before the bootloader, where the tiny vector table goes. From
/// version 2 on the reset vector is replaced with a jump to the bootloader,
/// and the table gets a jump to the program's start.
pub fn page_image(info: &Info, image: &[u8]) -> Result<Vec<(u16, Vec<u8>)>, String> {
    if image.len() > info.flash_size as usize {
        return Err(format!(
            "the program takes {} bytes, but the bootloader leaves {} bytes of flash",
            image.len(),
            info.flash_size
        ));
    }
    if image.len() < 4 {
        return Err("the program has no reset vector".into());
    }
    let page = info.page_size as usize;
    let start = info.bootloader_start as usize;
    let last = start - page;
    let mut pages = vec![];
    let user_reset = {
        let word0 = u16::from_le_bytes([image[0], image[1]]);
        let word1 = u16::from_le_bytes([image[2], image[3]]);
        if word0 == 0x940C {
            word1
        } else if word0 & 0xF000 == 0xC000 {
            // An rjmp's target, in words.
            (word0 & 0x0FFF).wrapping_add(1)
        } else {
            return Err(
                "the program does not start with a jump, which the bootloader needs to start it"
                    .into(),
            );
        }
    };
    for address in (0..start).step_by(page) {
        let mut bytes = vec![0xFFu8; page];
        let holds_data = address < image.len();
        if holds_data {
            let end = image.len().min(address + page);
            bytes[..end - address].copy_from_slice(&image[address..end]);
        }
        if info.major >= 2 {
            if address == 0 {
                let start_words = (start / 2) as u16;
                if start > 0x2000 {
                    bytes[..2].copy_from_slice(&0x940Cu16.to_le_bytes());
                    bytes[2..4].copy_from_slice(&start_words.to_le_bytes());
                } else {
                    let jump = 0xC000 | (start_words.wrapping_sub(1) & 0x0FFF);
                    bytes[..2].copy_from_slice(&jump.to_le_bytes());
                }
            }
            if address == last {
                if start > 0x2000 {
                    bytes[page - 4..page - 2].copy_from_slice(&0x940Cu16.to_le_bytes());
                    bytes[page - 2..].copy_from_slice(&user_reset.to_le_bytes());
                } else {
                    let jump = 0xC000 | (user_reset.wrapping_sub((start / 2) as u16) & 0x0FFF);
                    bytes[page - 2..].copy_from_slice(&jump.to_le_bytes());
                }
            }
        }
        if holds_data || address == last {
            pages.push((address as u16, bytes));
        }
    }
    Ok(pages)
}

/// Wait up to `wait` for the bootloader to answer, telling the user to
/// plug the board in if it does not at once. The Digispark's USB pull-up is
/// wired to its supply, so the computer may keep listing the bootloader
/// while the program runs; only an answer tells that it is there.
pub fn connect(description: &str, wait: Duration, quiet: bool) -> Result<(Device, Info), String> {
    let deadline = Instant::now() + wait;
    let mut told = false;
    loop {
        if let Some(device) = Device::open(VENDOR, PRODUCT)?
            && let Ok(info) = get_info(&device, PROBE_TIMEOUT)
        {
            return Ok((device, info));
        }
        if !told && !quiet {
            eprintln!(
                "Waiting for the {description}'s bootloader: plug the board in (or unplug it and plug it in again) within {} seconds",
                wait.as_secs()
            );
            told = true;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "no micronucleus bootloader appeared on USB within {} seconds; it runs for a few seconds after the {description} is plugged in",
                wait.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn get_info(device: &Device, timeout: Duration) -> Result<Info, String> {
    let mut reply = [0u8; 6];
    let count = device
        .control_in(GET_INFO, 0, 0, &mut reply, timeout)
        .map_err(|e| format!("cannot read the bootloader's information: {e}"))?;
    Info::parse(device.release, &reply[..count])
}

/// What a flash run wrote.
pub struct Written {
    pub info: Info,
    /// The image's size in bytes.
    pub bytes: usize,
}

/// Erase the application flash, write `image` (the flash contents from
/// address 0), and start it. `signature` is the chip's three signature
/// bytes and `page_size` its flash page size, which the bootloader must
/// match.
pub fn flash(
    description: &str,
    image: &[u8],
    signature: [u8; 3],
    page_size: u16,
    verbose: bool,
    quiet: bool,
) -> Result<Written, String> {
    let (mut device, info) = connect(description, Duration::from_secs(60), quiet)?;
    if verbose {
        eprintln!(
            "Micronucleus {}.{:02}: {} bytes of flash for programs, {}-byte pages",
            info.major, info.minor, info.flash_size, info.page_size
        );
    }
    if let Some(found) = info.signature
        && found != [signature[1], signature[2]]
    {
        return Err(format!(
            "the bootloader runs on a chip with signature 1E {:02X} {:02X}, not the {description}'s {:02X} {:02X} {:02X}",
            found[0], found[1], signature[0], signature[1], signature[2]
        ));
    }
    if info.page_size as u16 != page_size {
        return Err(format!(
            "the bootloader writes {}-byte pages, not the {description}'s {page_size}",
            info.page_size
        ));
    }
    let pages = page_image(&info, image)?;
    // The chip stops serving USB while it erases, so the request may fail
    // after it was received; the bootloader may also enumerate again.
    match device.control_out(ERASE, 0, 0, &[], TIMEOUT) {
        Ok(()) | Err(usb::Error::Failed(_)) | Err(usb::Error::Gone) => {}
    }
    std::thread::sleep(info.erase_sleep + Duration::from_millis(100));
    device = reconnect(device, &info)?;
    for (address, bytes) in &pages {
        write_page(&device, &info, *address, bytes)
            .map_err(|e| format!("cannot write flash at {address:#06x}: {e}"))?;
        std::thread::sleep(info.write_sleep);
    }
    // The bootloader leaves USB at once to start the program, so the
    // request may fail.
    let _ = device.control_out(RUN, 0, 0, &[], TIMEOUT);
    Ok(Written {
        info,
        bytes: image.len(),
    })
}

/// The bootloader after an erase: the same device if it still answers,
/// otherwise the one that enumerates again within a few seconds.
fn reconnect(device: Device, info: &Info) -> Result<Device, String> {
    if get_info(&device, TIMEOUT).is_ok() {
        return Ok(device);
    }
    device.close();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(device) = Device::open(VENDOR, PRODUCT)?
            && let Ok(again) = get_info(&device, TIMEOUT)
            && again.flash_size == info.flash_size
        {
            return Ok(device);
        }
        if Instant::now() >= deadline {
            return Err("the bootloader did not come back after erasing the flash".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn write_page(device: &Device, info: &Info, address: u16, bytes: &[u8]) -> Result<(), usb::Error> {
    let length = bytes.len() as u16;
    if info.major < 2 {
        return device.control_out(WRITE_PAGE, length, address, bytes, TIMEOUT);
    }
    // Version 2 takes the page four bytes at a time, in the request's value
    // and index fields, and writes it once it has all of it.
    device.control_out(WRITE_PAGE, length, address, &[], TIMEOUT)?;
    for word in bytes.chunks(4) {
        let first = u16::from_le_bytes([word[0], word[1]]);
        let second = u16::from_le_bytes([word[2], word[3]]);
        device.control_out(WRITE_WORDS, first, second, &[], TIMEOUT)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digispark(release: u16) -> Info {
        let mut reply = vec![0x17, 0x7C, 64, 5];
        if release >= 0x200 {
            reply = vec![0x19, 0xC0, 64, 5, 0x93, 0x0B];
        }
        Info::parse(release, &reply).unwrap()
    }

    #[test]
    fn info_parses_both_versions() {
        let one = digispark(0x0106);
        assert_eq!((one.major, one.minor), (1, 6));
        assert_eq!(one.flash_size, 6012);
        assert_eq!(one.bootloader_start, 6016);
        assert_eq!(one.erase_sleep, Duration::from_millis(5 * 94));
        assert_eq!(one.signature, None);
        let two = digispark(0x0205);
        assert_eq!(two.flash_size, 6592);
        assert_eq!(two.bootloader_start, 6592);
        assert_eq!(two.signature, Some([0x93, 0x0B]));
        assert!(Info::parse(0x0205, &[0x19, 0xC0, 64, 5]).is_err());
    }

    #[test]
    fn version_1_pages_are_the_image_and_the_last_page() {
        let info = digispark(0x0106);
        let mut image = vec![0x0E, 0xC0, 0x00, 0x00];
        image.resize(100, 0xAA);
        let pages = page_image(&info, &image).unwrap();
        let addresses: Vec<u16> = pages.iter().map(|page| page.0).collect();
        assert_eq!(addresses, [0, 64, 5952]);
        assert_eq!(&pages[0].1[..4], &[0x0E, 0xC0, 0, 0]);
        assert_eq!(pages[1].1[35], 0xAA);
        assert_eq!(pages[1].1[36], 0xFF);
        assert!(pages[2].1.iter().all(|&b| b == 0xFF));
        assert!(
            page_image(&info, &vec![0x0E; 6013])
                .unwrap_err()
                .contains("6012")
        );
    }

    #[test]
    fn version_2_pages_move_the_reset_vector_into_the_tiny_table() {
        let info = digispark(0x0205);
        // rjmp .+28: the program starts at word 15.
        let image = vec![0x0E, 0xC0, 0xFF, 0xCF];
        let pages = page_image(&info, &image).unwrap();
        let last = pages.last().unwrap();
        assert_eq!(last.0, 6592 - 64);
        // rjmp from 6592 to the bootloader at word 3296: offset 3295.
        assert_eq!(
            u16::from_le_bytes([pages[0].1[0], pages[0].1[1]]),
            0xC000 | 3295
        );
        assert_eq!(&pages[0].1[2..4], &[0xFF, 0xCF]);
        // The table's rjmp, at word 3295, reaches word 15 across the wrap.
        let jump = u16::from_le_bytes([last.1[62], last.1[63]]);
        assert_eq!(jump, 0xC000 | (15u16.wrapping_sub(3296) & 0x0FFF));
        assert!(
            page_image(&info, &[0, 0, 0, 0])
                .unwrap_err()
                .contains("jump")
        );
    }
}
