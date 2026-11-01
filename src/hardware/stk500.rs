//! Flashing AVR boards through a serial bootloader that speaks STK500
//! version 1, as Optiboot on the Arduino Uno does and as avrdude's `arduino`
//! programmer expects: the host pulses DTR, which resets the chip through a
//! capacitor on the board, and the bootloader answers commands for about a
//! second before it starts the program.
//!
//! A flash run syncs, checks the chip's signature, writes the image a flash
//! page at a time (the bootloader erases each page before it writes it),
//! reads every page back to verify it, and leaves programming mode, upon which
//! the bootloader starts the new program.
use super::serial::{self, UsbId};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// USB serial ports AVR boards with a serial bootloader use: the Arduino
/// Uno's ATmega16U2 (Arduino LLC and Arduino SRL IDs, revision 3 and earlier
/// boards), and the WCH CH340, FTDI FT232R and Silicon Labs CP210x adapters
/// of compatible boards.
pub const PORTS: &[UsbId] = &[
    UsbId {
        vendor: 0x2341,
        product: 0x0043,
    },
    UsbId {
        vendor: 0x2341,
        product: 0x0001,
    },
    UsbId {
        vendor: 0x2341,
        product: 0x0243,
    },
    UsbId {
        vendor: 0x2A03,
        product: 0x0043,
    },
    UsbId {
        vendor: 0x1A86,
        product: 0x7523,
    },
    UsbId {
        vendor: 0x0403,
        product: 0x6001,
    },
    UsbId {
        vendor: 0x10C4,
        product: 0xEA60,
    },
];

/// Overrides the port `dodo run` finds.
pub const PORT_VARIABLE: &str = "DODO_SERIAL_PORT";

const STK_OK: u8 = 0x10;
const STK_INSYNC: u8 = 0x14;
const CRC_EOP: u8 = 0x20;
const GET_SYNC: u8 = 0x30;
const GET_PARAMETER: u8 = 0x41;
const ENTER_PROGMODE: u8 = 0x50;
const LEAVE_PROGMODE: u8 = 0x51;
const LOAD_ADDRESS: u8 = 0x55;
const PROG_PAGE: u8 = 0x64;
const READ_PAGE: u8 = 0x74;
const READ_SIGN: u8 = 0x75;
const PARAMETER_SW_MAJOR: u8 = 0x81;
const PARAMETER_SW_MINOR: u8 = 0x82;

/// The serial port of the one connected board: `DODO_SERIAL_PORT` if set,
/// else the one port with a known USB ID. Errors if none or several are.
pub fn find_port(description: &str) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(PORT_VARIABLE).filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let mut ports: Vec<PathBuf> = PORTS
        .iter()
        .flat_map(|&id| serial::usb_serial_ports(id))
        .collect();
    ports.sort();
    ports.dedup();
    match ports.as_slice() {
        [port] => Ok(port.clone()),
        [] => Err(format!(
            "no {description} is connected; connect it over USB, or name its serial port with {PORT_VARIABLE}"
        )),
        _ => Err(format!(
            "several boards are connected ({}); connect one at a time, or name its port with {PORT_VARIABLE}",
            ports
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// What a flash run found and did, for the caller's message.
#[derive(Debug, PartialEq, Eq)]
pub struct Written {
    pub bytes: usize,
    /// The bootloader's software version, such as (4, 4) for the Uno's
    /// Optiboot.
    pub version: (u8, u8),
}

/// The bytes a link moves; a serial port, or a simulated bootloader in tests.
pub trait Transport {
    /// Pulse the reset line, so the board restarts into its bootloader.
    fn reset(&mut self) -> Result<(), String>;
    fn discard(&mut self);
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), String>;
    /// Read what has arrived, waiting up to `timeout` for the first byte;
    /// 0 if nothing arrived.
    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> Result<usize, String>;
}

/// Write `image`, the flash contents from address 0, to the board on `port`
/// at `baud`, check it, and start it. The chip must report `signature`;
/// `page_size` is its flash page in bytes, and the image must fit `capacity`.
pub fn flash(
    port: &Path,
    baud: u32,
    image: &[u8],
    signature: [u8; 3],
    page_size: u16,
    capacity: u32,
    verbose: bool,
) -> Result<Written, String> {
    let mut link = Link {
        transport: SerialPort::open(port, baud)?,
        name: port.display().to_string(),
    };
    link.program(image, signature, page_size, capacity, verbose)
}

pub struct Link<T: Transport> {
    pub transport: T,
    /// The port's name, for messages.
    pub name: String,
}

impl<T: Transport> Link<T> {
    pub fn program(
        &mut self,
        image: &[u8],
        signature: [u8; 3],
        page_size: u16,
        capacity: u32,
        verbose: bool,
    ) -> Result<Written, String> {
        if image.len() > capacity as usize {
            return Err(format!(
                "the firmware ({} bytes) does not fit the {capacity} bytes of flash below the bootloader",
                image.len()
            ));
        }
        self.sync()?;
        let major = self.parameter(PARAMETER_SW_MAJOR)?;
        let minor = self.parameter(PARAMETER_SW_MINOR)?;
        if verbose {
            eprintln!("Bootloader: STK500 version 1, software {major}.{minor}");
        }
        self.command(&[ENTER_PROGMODE], 0)?;
        let found = self.command(&[READ_SIGN], 3)?;
        if found != signature {
            let _ = self.command(&[LEAVE_PROGMODE], 0);
            return Err(format!(
                "the chip on {} has signature {}, not {}; is the right board selected?",
                self.name,
                hex(&found),
                hex(&signature)
            ));
        }
        let page = page_size as usize;
        let mut padded = image.to_vec();
        padded.resize(image.len().div_ceil(page) * page, 0xFF);
        let start = Instant::now();
        for (index, chunk) in padded.chunks(page).enumerate() {
            self.load_address(index * page)?;
            let mut request = vec![PROG_PAGE, (page >> 8) as u8, page as u8, b'F'];
            request.extend_from_slice(chunk);
            // Erasing and writing a page takes the bootloader about 4.5 ms.
            self.command_waiting(&request, 0, Duration::from_millis(500))?;
        }
        if verbose {
            eprintln!(
                "Wrote {} pages in {:.1} s",
                padded.len() / page,
                start.elapsed().as_secs_f64()
            );
        }
        for (index, chunk) in padded.chunks(page).enumerate() {
            self.load_address(index * page)?;
            let read = self.command(&[READ_PAGE, (page >> 8) as u8, page as u8, b'F'], page)?;
            if read != chunk {
                let offset = read
                    .iter()
                    .zip(chunk)
                    .position(|(a, b)| a != b)
                    .unwrap_or(0);
                return Err(format!(
                    "flash verification failed at address {:#06x}: wrote {:#04x}, read {:#04x}",
                    index * page + offset,
                    chunk[offset],
                    read[offset]
                ));
            }
        }
        // The bootloader starts the program shortly after it answers.
        self.command(&[LEAVE_PROGMODE], 0)?;
        Ok(Written {
            bytes: image.len(),
            version: (major, minor),
        })
    }

    /// Reset the board and sync with its bootloader. Optiboot first flashes
    /// the LED for about 375 ms, reading nothing, and the ATmega's UART holds
    /// only two received bytes: more would desynchronize it, upon which it
    /// starts the program. So each sync waits long enough for an answer
    /// before the next. The bootloader listens for about a second, so a few
    /// resets are tried.
    fn sync(&mut self) -> Result<(), String> {
        for _ in 0..3 {
            self.transport.reset()?;
            for wait in [600, 250, 250] {
                self.transport.discard();
                self.transport.write_all(&[GET_SYNC, CRC_EOP])?;
                if self.expect(0, Duration::from_millis(wait)).is_ok() {
                    // Answers to earlier attempts may still arrive.
                    std::thread::sleep(Duration::from_millis(30));
                    self.transport.discard();
                    if self.command(&[GET_SYNC], 0).is_ok() {
                        return Ok(());
                    }
                }
            }
        }
        Err(format!(
            "the bootloader on {} did not answer; is an Arduino Uno or compatible board with its bootloader connected? Press its reset button and run again",
            self.name
        ))
    }

    fn parameter(&mut self, parameter: u8) -> Result<u8, String> {
        Ok(self.command(&[GET_PARAMETER, parameter], 1)?[0])
    }

    /// LOAD_ADDRESS takes a word address in flash.
    fn load_address(&mut self, byte_address: usize) -> Result<(), String> {
        let word = byte_address / 2;
        self.command(&[LOAD_ADDRESS, word as u8, (word >> 8) as u8], 0)?;
        Ok(())
    }

    fn command(&mut self, request: &[u8], answer: usize) -> Result<Vec<u8>, String> {
        self.command_waiting(request, answer, Duration::from_millis(500))
    }

    fn command_waiting(
        &mut self,
        request: &[u8],
        answer: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        let mut bytes = request.to_vec();
        bytes.push(CRC_EOP);
        self.transport.write_all(&bytes)?;
        self.expect(answer, timeout).map_err(|error| {
            format!(
                "the bootloader on {} failed command {:#04x}: {error}",
                self.name, request[0]
            )
        })
    }

    /// Read STK_INSYNC, `answer` bytes, and STK_OK.
    fn expect(&mut self, answer: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        let mut reply = vec![0u8; answer + 2];
        let mut filled = 0;
        let deadline = Instant::now() + timeout;
        while filled < reply.len() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!(
                    "no answer (received {} of {} bytes)",
                    filled,
                    reply.len()
                ));
            }
            filled += self.transport.read(&mut reply[filled..], left)?;
            if filled > 0 && reply[0] != STK_INSYNC {
                return Err(format!("out of sync (received {:#04x})", reply[0]));
            }
        }
        if reply[answer + 1] != STK_OK {
            return Err(format!("failed (received {:#04x})", reply[answer + 1]));
        }
        Ok(reply[1..=answer].to_vec())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A raw serial port, 8N1, with control of DTR and RTS.
#[cfg(all(unix, feature = "llvm"))]
pub struct SerialPort {
    fd: libc::c_int,
}

#[cfg(all(unix, feature = "llvm"))]
impl SerialPort {
    pub fn open(path: &Path, baud: u32) -> Result<Self, String> {
        use std::os::unix::ffi::OsStrExt;
        // Declared here under their plain names: the libc crate binds them
        // to versioned glibc symbols, which the static release link cannot
        // resolve.
        unsafe extern "C" {
            fn cfsetispeed(settings: *mut libc::termios, speed: libc::speed_t) -> libc::c_int;
            fn cfsetospeed(settings: *mut libc::termios, speed: libc::speed_t) -> libc::c_int;
            fn cfmakeraw(settings: *mut libc::termios);
        }
        let speed = match baud {
            19_200 => libc::B19200,
            57_600 => libc::B57600,
            115_200 => libc::B115200,
            other => return Err(format!("unsupported bootloader baud rate {other}")),
        };
        let name =
            std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let failed = |action: &str| {
            let error = std::io::Error::last_os_error();
            let hint = if error.kind() == std::io::ErrorKind::PermissionDenied
                && cfg!(target_os = "linux")
            {
                "; add yourself to the group that owns it (usually dialout) and log in again"
            } else if error.raw_os_error() == Some(libc::EBUSY) {
                "; close the serial monitor or other program that has it open"
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
            settings.c_cflag &= !libc::CRTSCTS;
            settings.c_cc[libc::VMIN] = 0;
            settings.c_cc[libc::VTIME] = 0;
            if cfsetispeed(&mut settings, speed) != 0
                || cfsetospeed(&mut settings, speed) != 0
                || libc::tcsetattr(fd, libc::TCSANOW, &settings) != 0
            {
                return Err(failed("configure"));
            }
            Ok(port)
        }
    }

    /// Assert (true, driving the line low) or release DTR and RTS together.
    fn lines(&mut self, on: bool) -> Result<(), String> {
        let lines: libc::c_int = libc::TIOCM_DTR | libc::TIOCM_RTS;
        let request = if on { libc::TIOCMBIS } else { libc::TIOCMBIC };
        // SAFETY: TIOCMBIS and TIOCMBIC read one int from the pointer.
        if unsafe { libc::ioctl(self.fd, request as _, &lines) } != 0 {
            return Err(format!(
                "cannot set the control lines of the serial port: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
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
impl Transport for SerialPort {
    /// The board's reset input follows DTR's falling edge through a
    /// capacitor: release the lines, then assert them.
    fn reset(&mut self) -> Result<(), String> {
        self.lines(false)?;
        std::thread::sleep(Duration::from_millis(250));
        self.lines(true)?;
        // Optiboot starts within a few milliseconds of reset.
        std::thread::sleep(Duration::from_millis(50));
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
pub struct SerialPort;

#[cfg(not(all(unix, feature = "llvm")))]
impl SerialPort {
    pub fn open(path: &Path, _baud: u32) -> Result<Self, String> {
        Err(format!(
            "cannot flash through {} on this system; write the .hex image with avrdude -c arduino",
            path.display()
        ))
    }
}

#[cfg(not(all(unix, feature = "llvm")))]
impl Transport for SerialPort {
    fn reset(&mut self) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Optiboot as a state machine over its flash, answering whole commands.
    struct Optiboot {
        flash: Vec<u8>,
        signature: [u8; 3],
        address: usize,
        pending: VecDeque<u8>,
        input: Vec<u8>,
        resets: usize,
        /// Bytes of noise sent after a reset, as a running program might.
        noise: Vec<u8>,
        left_progmode: bool,
        corrupt_at: Option<usize>,
    }

    impl Optiboot {
        fn new() -> Self {
            Self {
                flash: vec![0xFF; 32 * 1024],
                signature: [0x1E, 0x95, 0x0F],
                address: 0,
                pending: VecDeque::new(),
                input: vec![],
                resets: 0,
                noise: vec![],
                left_progmode: false,
                corrupt_at: None,
            }
        }

        fn answer(&mut self, bytes: &[u8]) {
            self.pending.push_back(STK_INSYNC);
            self.pending.extend(bytes);
            self.pending.push_back(STK_OK);
        }

        /// Handle complete commands in `input`.
        fn run(&mut self) {
            loop {
                let Some(&command) = self.input.first() else {
                    return;
                };
                let length = match command {
                    GET_SYNC | ENTER_PROGMODE | LEAVE_PROGMODE | READ_SIGN => 2,
                    GET_PARAMETER => 3,
                    LOAD_ADDRESS => 4,
                    READ_PAGE => 5,
                    PROG_PAGE => {
                        if self.input.len() < 3 {
                            return;
                        }
                        5 + ((self.input[1] as usize) << 8 | self.input[2] as usize)
                    }
                    _ => panic!("unexpected command {command:#04x}"),
                };
                if self.input.len() < length {
                    return;
                }
                let request: Vec<u8> = self.input.drain(..length).collect();
                assert_eq!(*request.last().unwrap(), CRC_EOP);
                match command {
                    GET_SYNC | ENTER_PROGMODE => self.answer(&[]),
                    LEAVE_PROGMODE => {
                        self.left_progmode = true;
                        self.answer(&[]);
                    }
                    READ_SIGN => {
                        let signature = self.signature;
                        self.answer(&signature);
                    }
                    GET_PARAMETER => match request[1] {
                        PARAMETER_SW_MAJOR => self.answer(&[4]),
                        PARAMETER_SW_MINOR => self.answer(&[4]),
                        _ => self.answer(&[3]),
                    },
                    LOAD_ADDRESS => {
                        self.address = (request[1] as usize | (request[2] as usize) << 8) * 2;
                        self.answer(&[]);
                    }
                    PROG_PAGE => {
                        assert_eq!(request[3], b'F');
                        let data = &request[4..request.len() - 1];
                        self.flash[self.address..self.address + data.len()].copy_from_slice(data);
                        if let Some(at) = self.corrupt_at
                            && (self.address..self.address + data.len()).contains(&at)
                        {
                            self.flash[at] ^= 1;
                        }
                        self.answer(&[]);
                    }
                    READ_PAGE => {
                        let length = (request[1] as usize) << 8 | request[2] as usize;
                        let data = self.flash[self.address..self.address + length].to_vec();
                        self.answer(&data);
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    impl Transport for Optiboot {
        fn reset(&mut self) -> Result<(), String> {
            self.resets += 1;
            self.input.clear();
            self.pending.clear();
            self.pending.extend(self.noise.clone());
            Ok(())
        }
        fn discard(&mut self) {
            self.pending.clear();
        }
        fn write_all(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.input.extend_from_slice(bytes);
            self.run();
            Ok(())
        }
        fn read(&mut self, buffer: &mut [u8], _: Duration) -> Result<usize, String> {
            let mut count = 0;
            while count < buffer.len() {
                let Some(byte) = self.pending.pop_front() else {
                    break;
                };
                buffer[count] = byte;
                count += 1;
            }
            Ok(count)
        }
    }

    fn link(board: Optiboot) -> Link<Optiboot> {
        Link {
            transport: board,
            name: "test".into(),
        }
    }

    const SIGNATURE: [u8; 3] = [0x1E, 0x95, 0x0F];

    #[test]
    fn programs_pads_verifies_and_starts_the_image() {
        let mut board = link(Optiboot::new());
        let image: Vec<u8> = (0..300u32).map(|i| i as u8).collect();
        let written = board.program(&image, SIGNATURE, 128, 32256, false).unwrap();
        assert_eq!(
            written,
            Written {
                bytes: 300,
                version: (4, 4)
            }
        );
        let flash = &board.transport.flash;
        assert_eq!(&flash[..300], &image[..]);
        assert!(flash[300..384].iter().all(|&b| b == 0xFF));
        assert!(board.transport.left_progmode);
        assert_eq!(board.transport.resets, 1);
    }

    #[test]
    fn syncs_past_noise_from_the_running_program() {
        let mut optiboot = Optiboot::new();
        optiboot.noise = b"hello from the old program".to_vec();
        let mut board = link(optiboot);
        board
            .program(&[1, 2, 3], SIGNATURE, 128, 32256, false)
            .unwrap();
        assert_eq!(&board.transport.flash[..3], &[1, 2, 3]);
    }

    #[test]
    fn rejects_another_chip_and_oversized_images() {
        let mut optiboot = Optiboot::new();
        optiboot.signature = [0x1E, 0x98, 0x01];
        let error = link(optiboot)
            .program(&[0; 4], SIGNATURE, 128, 32256, false)
            .unwrap_err();
        assert!(
            error.contains("signature 1E 98 01, not 1E 95 0F"),
            "{error}"
        );
        let error = link(Optiboot::new())
            .program(&[0; 40_000], SIGNATURE, 128, 32256, false)
            .unwrap_err();
        assert!(error.contains("does not fit"), "{error}");
    }

    #[test]
    fn reports_a_page_that_reads_back_wrong() {
        let mut optiboot = Optiboot::new();
        optiboot.corrupt_at = Some(130);
        let error = link(optiboot)
            .program(&[7; 200], SIGNATURE, 128, 32256, false)
            .unwrap_err();
        assert!(
            error.contains("verification failed at address 0x0082"),
            "{error}"
        );
    }

    struct Silent;
    impl Transport for Silent {
        fn reset(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn discard(&mut self) {}
        fn write_all(&mut self, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn read(&mut self, _: &mut [u8], timeout: Duration) -> Result<usize, String> {
            std::thread::sleep(timeout.min(Duration::from_millis(1)));
            Ok(0)
        }
    }

    #[test]
    fn gives_up_when_no_bootloader_answers() {
        let mut board = Link {
            transport: Silent,
            name: "/dev/test".into(),
        };
        let error = board
            .program(&[0; 4], SIGNATURE, 128, 32256, false)
            .unwrap_err();
        assert!(
            error.contains("bootloader on /dev/test did not answer"),
            "{error}"
        );
    }
}
