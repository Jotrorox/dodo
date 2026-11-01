//! Flashing Espressif chips through their boot ROM's serial flasher, the
//! protocol esptool speaks: SLIP-framed commands that sync, erase, write in
//! 1 KiB blocks, and check the result with the ROM's MD5 command. The host
//! resets the chip into the ROM's download mode itself, by toggling the
//! port's DTR and RTS lines: the chip's own USB serial port (USB-Serial-JTAG)
//! decodes them in hardware, and development boards wire a USB serial
//! adapter's lines to the EN and boot pins.
//!
//! Only the ROM is used, no flasher stub uploaded to RAM, so nothing but the
//! image travels over the port.
use super::serial::{self, UsbId};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// USB serial adapters Espressif development boards use, which the
/// download mode reset reaches through the adapter's DTR and RTS lines:
/// Silicon Labs CP210x and WCH CH340, CH343, and CH9102.
const ADAPTERS: &[UsbId] = &[
    UsbId {
        vendor: 0x10C4,
        product: 0xEA60,
    },
    UsbId {
        vendor: 0x1A86,
        product: 0x7523,
    },
    UsbId {
        vendor: 0x1A86,
        product: 0x55D3,
    },
    UsbId {
        vendor: 0x1A86,
        product: 0x55D4,
    },
];

/// Overrides the port `dodo run` finds.
pub const PORT_VARIABLE: &str = "DODO_SERIAL_PORT";

/// Bytes per FLASH_DATA block, the ROM's limit.
const BLOCK: usize = 0x400;
const SECTOR: usize = 0x1000;

/// A port to flash through, and whether it is the chip's own USB serial
/// port, which resets differently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    pub path: PathBuf,
    pub native: bool,
}

/// The port to flash an Espressif chip with USB serial port `usb` through:
/// `DODO_SERIAL_PORT` if set; else the chip's own USB serial port; else a
/// USB serial adapter. Errors if none or several are connected.
pub fn find_port(usb: UsbId, description: &str) -> Result<Port, String> {
    if let Some(path) = std::env::var_os(PORT_VARIABLE).filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        let native = serial::usb_serial_ports(usb).contains(&path);
        return Ok(Port { path, native });
    }
    let choose = |ports: Vec<PathBuf>, native: bool| -> Result<Option<Port>, String> {
        match ports.as_slice() {
            [] => Ok(None),
            [path] => Ok(Some(Port {
                path: path.clone(),
                native,
            })),
            _ => Err(format!(
                "several boards are connected ({}); connect one at a time, or name its port with {PORT_VARIABLE}",
                ports
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    };
    if let Some(port) = choose(serial::usb_serial_ports(usb), true)? {
        return Ok(port);
    }
    let adapters = ADAPTERS
        .iter()
        .flat_map(|&id| serial::usb_serial_ports(id))
        .collect();
    if let Some(port) = choose(adapters, false)? {
        return Ok(port);
    }
    Err(format!(
        "no {description} is connected; connect it over USB (either connector of a development board), or name its serial port with {PORT_VARIABLE}"
    ))
}

/// What a flash run did, for the caller's message.
pub struct Written {
    pub bytes: usize,
    /// The flash already held the image, so nothing was erased or written.
    pub unchanged: bool,
}

/// A register bit that, while set, makes every reset start the boot ROM's
/// download mode instead of the firmware, such as the ESP32-C6's
/// LP_AON_SYS_CFG.FORCE_DOWNLOAD_BOOT, which firmware sets to restart into
/// the bootloader; and the register writes, `(address, value, mask)`, that
/// stop the watchdogs a reset into download mode leaves running.
#[derive(Clone, Copy, Debug)]
pub struct DownloadFlag {
    pub address: u32,
    pub mask: u32,
    /// The reset through the chip's own USB serial port resets only the
    /// digital core, so a watchdog in the low-power domain that firmware
    /// started keeps counting in download mode and would reset the chip
    /// while it is flashed, as esptool also knows.
    pub watchdogs_off: &'static [(u32, u32, u32)],
}

/// Write `image` to flash offset 0 of the Espressif chip with ID
/// `chip_id` on `port`, check it, clear `download_flag`, and restart the
/// chip into the image.
pub fn flash(
    port: &Port,
    image: &[u8],
    chip_id: u16,
    flash_size: u32,
    download_flag: DownloadFlag,
    verbose: bool,
) -> Result<Written, String> {
    if image.len() > flash_size as usize {
        return Err(format!(
            "the firmware ({} KiB) does not fit the {} KiB flash",
            image.len() / 1024,
            flash_size / 1024
        ));
    }
    let mut link = Link::open(&port.path)?;
    // Stop the watchdogs first, before a running one resets the chip out of
    // download mode; if one does, start over.
    let mut attempts = 0;
    let found = loop {
        attempts += 1;
        link.enter_download_mode(port.native)?;
        match link.stop_watchdogs(download_flag.watchdogs_off) {
            Ok(()) => break link.chip_id()?,
            Err(error) if attempts >= 4 => return Err(error),
            Err(_) => {}
        }
    };
    if found != chip_id as u32 {
        return Err(format!(
            "the chip on {} has ID {found}, not {chip_id}; is the right board connected?",
            port.path.display()
        ));
    }
    // A restart into the bootloader leaves the flag set, and with it the
    // chip would return to download mode at the reset below.
    let (flags, _) = link.command(
        READ_REG,
        &download_flag.address.to_le_bytes(),
        Duration::from_secs(3),
    )?;
    if flags & download_flag.mask != 0 {
        let mut write = vec![];
        for word in [
            download_flag.address,
            flags & !download_flag.mask,
            u32::MAX,
            0,
        ] {
            write.extend_from_slice(&word.to_le_bytes());
        }
        link.command(WRITE_REG, &write, Duration::from_secs(3))?;
    }
    link.command(SPI_ATTACH, &[0; 8], Duration::from_secs(3))?;
    let mut params = vec![];
    for word in [0, flash_size, 0x10000, SECTOR as u32, 256, 0xFFFF] {
        params.extend_from_slice(&word.to_le_bytes());
    }
    link.command(SPI_SET_PARAMS, &params, Duration::from_secs(3))?;
    // Whole sectors, padded as erased flash, so the MD5 covers what the
    // erase changed.
    let mut padded = image.to_vec();
    padded.resize(image.len().div_ceil(SECTOR) * SECTOR, 0xFF);
    let expected = md5(&padded);
    if link.flash_md5(0, padded.len() as u32)? == expected {
        link.run_firmware(port.native)?;
        return Ok(Written {
            bytes: image.len(),
            unchanged: true,
        });
    }
    let blocks = padded.len().div_ceil(BLOCK);
    let mut begin = vec![];
    for word in [padded.len() as u32, blocks as u32, BLOCK as u32, 0, 0] {
        begin.extend_from_slice(&word.to_le_bytes());
    }
    link.command(FLASH_BEGIN, &begin, per_megabyte(padded.len(), 30))?;
    for (sequence, chunk) in padded.chunks(BLOCK).enumerate() {
        if verbose {
            eprint!("\rWriting block {}/{blocks}", sequence + 1);
        }
        let mut block = chunk.to_vec();
        block.resize(BLOCK, 0xFF);
        let mut data = vec![];
        for word in [BLOCK as u32, sequence as u32, 0, 0] {
            data.extend_from_slice(&word.to_le_bytes());
        }
        data.extend_from_slice(&block);
        link.command_checked(FLASH_DATA, &data, checksum(&block), Duration::from_secs(10))?;
    }
    if verbose {
        eprintln!();
    }
    if link.flash_md5(0, padded.len() as u32)? != expected {
        return Err(format!(
            "the flash on {} does not hold the image after writing it; try again",
            port.path.display()
        ));
    }
    // Stay in the ROM: the reset below starts the firmware.
    link.command(FLASH_END, &1u32.to_le_bytes(), Duration::from_secs(3))?;
    link.run_firmware(port.native)?;
    Ok(Written {
        bytes: image.len(),
        unchanged: false,
    })
}

/// A timeout of `seconds` per MB of `bytes`, at least 3 seconds.
fn per_megabyte(bytes: usize, seconds: u64) -> Duration {
    Duration::from_millis((bytes as u64 * seconds * 1000 / 1_000_000).max(3000))
}

const FLASH_BEGIN: u8 = 0x02;
const FLASH_DATA: u8 = 0x03;
const FLASH_END: u8 = 0x04;
const SYNC: u8 = 0x08;
const WRITE_REG: u8 = 0x09;
const READ_REG: u8 = 0x0A;
const SPI_SET_PARAMS: u8 = 0x0B;
const SPI_ATTACH: u8 = 0x0D;
const SPI_FLASH_MD5: u8 = 0x13;
const GET_SECURITY_INFO: u8 = 0x14;

/// The checksum FLASH_DATA carries: the XOR of the data bytes and 0xEF.
pub fn checksum(data: &[u8]) -> u32 {
    data.iter().fold(0xEF, |sum, &byte| sum ^ byte) as u32
}

/// SLIP framing: 0xC0 ends a frame, and 0xC0 and 0xDB inside one are sent as
/// 0xDB 0xDC and 0xDB 0xDD.
pub fn slip_encode(packet: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet.len() + 2);
    out.push(0xC0);
    for &byte in packet {
        match byte {
            0xC0 => out.extend_from_slice(&[0xDB, 0xDC]),
            0xDB => out.extend_from_slice(&[0xDB, 0xDD]),
            _ => out.push(byte),
        }
    }
    out.push(0xC0);
    out
}

/// Collects SLIP frames from bytes as they arrive.
#[derive(Default)]
pub struct SlipDecoder {
    frame: Vec<u8>,
    in_frame: bool,
    escape: bool,
}

impl SlipDecoder {
    /// Feed one byte; returns a frame once its end arrives.
    pub fn push(&mut self, byte: u8) -> Option<Vec<u8>> {
        if byte == 0xC0 {
            if self.in_frame && !self.frame.is_empty() {
                self.in_frame = false;
                return Some(std::mem::take(&mut self.frame));
            }
            // A start, or an empty frame between two delimiters.
            self.in_frame = true;
            self.frame.clear();
            return None;
        }
        if !self.in_frame {
            // Boot messages and other text outside frames.
            return None;
        }
        if self.escape {
            self.escape = false;
            self.frame.push(match byte {
                0xDC => 0xC0,
                0xDD => 0xDB,
                other => other,
            });
        } else if byte == 0xDB {
            self.escape = true;
        } else {
            self.frame.push(byte);
        }
        None
    }
}

/// A command packet: direction 0, the command, the data length, a checksum
/// (FLASH_DATA's only), then the data.
pub fn command_packet(command: u8, data: &[u8], checksum: u32) -> Vec<u8> {
    let mut packet = vec![0, command];
    packet.extend_from_slice(&(data.len() as u16).to_le_bytes());
    packet.extend_from_slice(&checksum.to_le_bytes());
    packet.extend_from_slice(data);
    packet
}

/// A response to `command`: its value word and data, with the ROM's four
/// status bytes (status, error, and two reserved) removed. Err for a
/// response to another command or one the ROM reports failed.
pub fn parse_response(frame: &[u8], command: u8) -> Option<Result<(u32, Vec<u8>), String>> {
    if frame.len() < 8 || frame[0] != 1 || frame[1] != command {
        return None;
    }
    let length = u16::from_le_bytes([frame[2], frame[3]]) as usize;
    let value = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let data = &frame[8..];
    if data.len() < length || length < 2 {
        return Some(Err(format!("malformed response to command {command:#04x}")));
    }
    let status_at = if length >= 4 { length - 4 } else { length - 2 };
    let (status, error) = (data[status_at], data[status_at + 1]);
    if status != 0 {
        let reason = match error {
            0x05 => "invalid message",
            0x06 => "failed to act",
            0x07 => "invalid CRC",
            0x08 => "flash write error",
            0x09 => "flash read error",
            0x0A => "flash read length error",
            0x0B => "deflate error",
            _ => "error",
        };
        return Some(Err(format!(
            "the boot ROM refused command {command:#04x}: {reason} ({error:#04x})"
        )));
    }
    Some(Ok((value, data[..status_at].to_vec())))
}

struct Link {
    port: SerialPort,
    decoder: SlipDecoder,
    path: PathBuf,
}

impl Link {
    fn open(path: &Path) -> Result<Self, String> {
        Ok(Self {
            port: SerialPort::open(path)?,
            decoder: SlipDecoder::default(),
            path: path.to_path_buf(),
        })
    }

    /// Reset the chip with its boot pin low, so the ROM waits for commands,
    /// and sync with it. Retries a few times, as the first sync often
    /// meets the ROM's boot message.
    fn enter_download_mode(&mut self, native: bool) -> Result<(), String> {
        let mut sync = vec![0x07, 0x07, 0x12, 0x20];
        sync.extend_from_slice(&[0x55; 32]);
        for attempt in 0..4 {
            if native {
                // USB-Serial-JTAG: RTS resets the chip and DTR holds the
                // boot pin low, decoded from the pair; passing through
                // (1, 1) instead of (0, 0) avoids a stray reset.
                self.port.lines(false, false)?;
                sleep_ms(100);
                self.port.lines(true, false)?;
                sleep_ms(100);
                self.port.lines(false, true)?;
                sleep_ms(100);
                self.port.lines(false, false)?;
            } else {
                // An adapter wired as on Espressif's boards: RTS drives EN
                // (reset) and DTR the boot pin, each through a transistor.
                self.port.lines(false, true)?;
                sleep_ms(100);
                self.port.lines(true, false)?;
                sleep_ms(if attempt % 2 == 0 { 50 } else { 550 });
                self.port.lines(false, false)?;
            }
            self.port.discard();
            self.decoder = SlipDecoder::default();
            for _ in 0..5 {
                self.send(SYNC, &sync, 0)?;
                if self.receive(SYNC, Duration::from_millis(100)).is_ok() {
                    // The ROM answers each SYNC several times; drop the rest.
                    sleep_ms(50);
                    self.port.discard();
                    self.decoder = SlipDecoder::default();
                    return Ok(());
                }
            }
        }
        Err(format!(
            "the chip on {} did not enter its download mode; hold the BOOT button while pressing RESET, then run again",
            self.path.display()
        ))
    }

    /// Write `(address, value, mask)` triples, each answered within 500 ms.
    fn stop_watchdogs(&mut self, writes: &[(u32, u32, u32)]) -> Result<(), String> {
        for &(address, value, mask) in writes {
            let mut write = vec![];
            for word in [address, value, mask, 0] {
                write.extend_from_slice(&word.to_le_bytes());
            }
            self.command(WRITE_REG, &write, Duration::from_millis(500))?;
        }
        Ok(())
    }

    fn chip_id(&mut self) -> Result<u32, String> {
        let (_, data) = self.command(GET_SECURITY_INFO, &[], Duration::from_secs(3))?;
        // Flags (4), flash encryption count (1), key purposes (7), chip ID.
        data.get(12..16)
            .map(|id| u32::from_le_bytes([id[0], id[1], id[2], id[3]]))
            .ok_or_else(|| "the boot ROM did not report its chip".to_string())
    }

    /// The ROM's MD5 of `size` bytes of flash at `offset`, as 32 hex digits.
    fn flash_md5(&mut self, offset: u32, size: u32) -> Result<[u8; 16], String> {
        let mut data = vec![];
        for word in [offset, size, 0, 0] {
            data.extend_from_slice(&word.to_le_bytes());
        }
        let (_, digest) = self.command(SPI_FLASH_MD5, &data, per_megabyte(size as usize, 8))?;
        let text = std::str::from_utf8(digest.get(..32).unwrap_or_default())
            .map_err(|_| "malformed MD5 from the boot ROM".to_string())?;
        let mut out = [0u8; 16];
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[2 * index..2 * index + 2], 16)
                .map_err(|_| "malformed MD5 from the boot ROM".to_string())?;
        }
        Ok(out)
    }

    /// Reset the chip with its boot pin high, so it starts the firmware.
    fn run_firmware(&mut self, native: bool) -> Result<(), String> {
        sleep_ms(100);
        if native {
            self.port.lines(false, false)?;
            sleep_ms(100);
            self.port.lines(false, true)?;
            sleep_ms(100);
            self.port.lines(false, false)?;
        } else {
            self.port.lines(false, true)?;
            sleep_ms(100);
            self.port.lines(false, false)?;
        }
        Ok(())
    }

    fn command(
        &mut self,
        command: u8,
        data: &[u8],
        timeout: Duration,
    ) -> Result<(u32, Vec<u8>), String> {
        self.command_checked(command, data, 0, timeout)
    }

    fn command_checked(
        &mut self,
        command: u8,
        data: &[u8],
        checksum: u32,
        timeout: Duration,
    ) -> Result<(u32, Vec<u8>), String> {
        self.send(command, data, checksum)?;
        self.receive(command, timeout)
    }

    fn send(&mut self, command: u8, data: &[u8], checksum: u32) -> Result<(), String> {
        self.port
            .write_all(&slip_encode(&command_packet(command, data, checksum)))
    }

    fn receive(&mut self, command: u8, timeout: Duration) -> Result<(u32, Vec<u8>), String> {
        let deadline = std::time::Instant::now() + timeout;
        let mut buffer = [0u8; 256];
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err(format!(
                    "no answer from the boot ROM on {} to command {command:#04x}",
                    self.path.display()
                ));
            }
            let count = self.port.read(&mut buffer, left)?;
            for &byte in &buffer[..count] {
                if let Some(frame) = self.decoder.push(byte)
                    && let Some(result) = parse_response(&frame, command)
                {
                    return result;
                }
            }
        }
    }
}

fn sleep_ms(milliseconds: u64) {
    std::thread::sleep(Duration::from_millis(milliseconds));
}

/// A raw serial port at 115200 baud, 8N1, with control of DTR and RTS.
#[cfg(all(unix, feature = "llvm"))]
struct SerialPort {
    fd: libc::c_int,
}

#[cfg(all(unix, feature = "llvm"))]
impl SerialPort {
    fn open(path: &Path) -> Result<Self, String> {
        use std::os::unix::ffi::OsStrExt;
        // Declared here under their plain names: the libc crate binds them
        // to versioned glibc symbols, which the static release link cannot
        // resolve.
        unsafe extern "C" {
            fn cfsetispeed(settings: *mut libc::termios, speed: libc::speed_t) -> libc::c_int;
            fn cfsetospeed(settings: *mut libc::termios, speed: libc::speed_t) -> libc::c_int;
            fn cfmakeraw(settings: *mut libc::termios);
        }
        let name =
            std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let failed = |action: &str| {
            let error = std::io::Error::last_os_error();
            let hint = if error.kind() == std::io::ErrorKind::PermissionDenied
                && cfg!(target_os = "linux")
            {
                "; add yourself to the group that owns it (usually dialout) and log in again"
            } else {
                ""
            };
            format!("cannot {action} {}: {error}{hint}", path.display())
        };
        // SAFETY: plain POSIX calls on a descriptor this function owns, with
        // a termios structure that tcgetattr fills before it is used.
        unsafe {
            let fd = libc::open(
                name.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK,
            );
            if fd < 0 {
                return Err(failed("open"));
            }
            let port = Self { fd };
            let mut settings: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut settings) != 0 {
                return Err(failed("read the settings of"));
            }
            cfmakeraw(&mut settings);
            settings.c_cflag |= libc::CLOCAL | libc::CREAD;
            // Leave DTR and RTS as set when the port closes, so closing it
            // does not reset the chip.
            settings.c_cflag &= !(libc::HUPCL | libc::CRTSCTS);
            settings.c_cc[libc::VMIN] = 0;
            settings.c_cc[libc::VTIME] = 0;
            if cfsetispeed(&mut settings, libc::B115200) != 0
                || cfsetospeed(&mut settings, libc::B115200) != 0
                || libc::tcsetattr(fd, libc::TCSANOW, &settings) != 0
            {
                return Err(failed("configure"));
            }
            Ok(port)
        }
    }

    /// Set DTR and RTS: true asserts the line, which drives it low. Lines
    /// are asserted before others are released, so a change of both passes
    /// through both asserted, which neither reset circuit acts on.
    fn lines(&mut self, dtr: bool, rts: bool) -> Result<(), String> {
        let mut changes = [(libc::TIOCM_DTR, dtr), (libc::TIOCM_RTS, rts)];
        changes.sort_by_key(|&(_, on)| !on);
        for (line, on) in changes {
            let request = if on { libc::TIOCMBIS } else { libc::TIOCMBIC };
            // SAFETY: TIOCMBIS and TIOCMBIC read one int from the pointer.
            if unsafe { libc::ioctl(self.fd, request as _, &line) } != 0 {
                return Err(format!(
                    "cannot set the control lines of the serial port: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(())
    }

    fn discard(&mut self) {
        // SAFETY: flushes the descriptor's queues.
        unsafe {
            libc::tcflush(self.fd, libc::TCIOFLUSH);
        }
    }

    fn write_all(&mut self, mut bytes: &[u8]) -> Result<(), String> {
        while !bytes.is_empty() {
            // SAFETY: writes from a live slice.
            let written = unsafe { libc::write(self.fd, bytes.as_ptr().cast(), bytes.len()) };
            if written < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    self.wait(libc::POLLOUT, Duration::from_secs(1))?;
                    continue;
                }
                return Err(format!("cannot write to the serial port: {error}"));
            }
            bytes = &bytes[written as usize..];
        }
        Ok(())
    }

    /// Read what has arrived, waiting up to `timeout` for the first byte.
    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> Result<usize, String> {
        if !self.wait(libc::POLLIN, timeout)? {
            return Ok(0);
        }
        // SAFETY: reads into a live buffer.
        let count = unsafe { libc::read(self.fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(0);
            }
            return Err(format!("cannot read from the serial port: {error}"));
        }
        if count == 0 {
            return Err("the serial port closed; was the board disconnected?".into());
        }
        Ok(count as usize)
    }

    fn wait(&mut self, events: libc::c_short, timeout: Duration) -> Result<bool, String> {
        let mut entry = libc::pollfd {
            fd: self.fd,
            events,
            revents: 0,
        };
        let milliseconds = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
        // SAFETY: polls one live descriptor.
        let ready = unsafe { libc::poll(&mut entry, 1, milliseconds) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(format!("cannot wait for the serial port: {error}"));
        }
        if ready > 0 && (entry.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
            return Err("the serial port closed; was the board disconnected?".into());
        }
        Ok(ready > 0)
    }
}

#[cfg(all(unix, feature = "llvm"))]
impl Drop for SerialPort {
    fn drop(&mut self) {
        // SAFETY: closes the descriptor this value owns.
        unsafe {
            libc::close(self.fd);
        }
    }
}

/// Without POSIX terminals the port cannot be driven from here.
#[cfg(not(all(unix, feature = "llvm")))]
struct SerialPort;

#[cfg(not(all(unix, feature = "llvm")))]
impl SerialPort {
    fn open(path: &Path) -> Result<Self, String> {
        Err(format!(
            "cannot flash through {} on this system; flash the .bin image at offset 0 with esptool or espflash",
            path.display()
        ))
    }
    fn lines(&mut self, _: bool, _: bool) -> Result<(), String> {
        unreachable!()
    }
    fn discard(&mut self) {}
    fn write_all(&mut self, _: &[u8]) -> Result<(), String> {
        unreachable!()
    }
    fn read(&mut self, _: &mut [u8], _: Duration) -> Result<usize, String> {
        unreachable!()
    }
}

/// MD5 (RFC 1321), which the boot ROM's SPI_FLASH_MD5 command reports.
pub fn md5(data: &[u8]) -> [u8; 16] {
    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let constants: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32)
        .collect();
    let mut state: [u32; 4] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_le_bytes());
    for block in message.chunks(64) {
        let words: Vec<u32> = block
            .chunks(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(constants[i])
                .wrapping_add(words[g])
                .rotate_left(SHIFTS[i]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(rotated);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (index, word) in state.iter().enumerate() {
        out[4 * index..4 * index + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_rfc_1321() {
        let hex = |digest: [u8; 16]| {
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(md5(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            )),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn slip_frames_round_trip_with_escapes() {
        let packet = [1, 0xC0, 2, 0xDB, 3];
        let encoded = slip_encode(&packet);
        assert_eq!(encoded, [0xC0, 1, 0xDB, 0xDC, 2, 0xDB, 0xDD, 3, 0xC0]);
        let mut decoder = SlipDecoder::default();
        // Text before the frame, as the ROM's boot message, is skipped.
        let mut frames = vec![];
        for &byte in b"ESP-ROM".iter().chain(&encoded) {
            frames.extend(decoder.push(byte));
        }
        assert_eq!(frames, [packet.to_vec()]);
    }

    #[test]
    fn responses_carry_value_data_and_status() {
        let ok = [1, 0x14, 8, 0, 0xAA, 0, 0, 0, 7, 8, 9, 10, 0, 0, 0, 0];
        let (value, data) = parse_response(&ok, 0x14).unwrap().unwrap();
        assert_eq!((value, data), (0xAA, vec![7, 8, 9, 10]));
        assert!(parse_response(&ok, 0x13).is_none());
        let refused = [1, 0x02, 4, 0, 0, 0, 0, 0, 1, 0x08, 0, 0];
        assert!(
            parse_response(&refused, 0x02)
                .unwrap()
                .unwrap_err()
                .contains("flash write error")
        );
    }

    #[test]
    fn commands_carry_length_and_checksum() {
        let data = [1u8, 2, 3];
        assert_eq!(checksum(&data), 0xEF ^ 1 ^ 2 ^ 3);
        assert_eq!(
            command_packet(FLASH_DATA, &data, 0x12),
            [0, 3, 3, 0, 0x12, 0, 0, 0, 1, 2, 3]
        );
    }
}
