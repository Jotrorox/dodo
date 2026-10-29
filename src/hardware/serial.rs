//! USB serial ports of attached boards, and restarting a board into its
//! bootloader through one: firmware that opens the chip package's USB
//! serial port (`std/embedded/chip/rp_usb`) restarts into the bootloader
//! when the host sets the port to 1200 baud, so `dodo run` needs no BOOTSEL
//! button press.
use std::path::{Path, PathBuf};

/// The USB vendor and product IDs of a board's serial port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbId {
    pub vendor: u16,
    pub product: u16,
}

/// The baud rate that asks the firmware to restart into its bootloader.
pub const BOOTLOADER_BAUD: u32 = 1200;

/// Serial ports of attached USB devices with `id`, sorted. Linux and macOS
/// only; elsewhere none.
pub fn usb_serial_ports(id: UsbId) -> Vec<PathBuf> {
    let mut ports = if cfg!(target_os = "linux") {
        linux_ports(id)
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("ioreg")
            .args(["-r", "-c", "IOUSBHostDevice", "-l", "-w0"])
            .output()
            .map(|output| ioreg_ports(&String::from_utf8_lossy(&output.stdout), id))
            .unwrap_or_default()
    } else {
        vec![]
    };
    ports.sort();
    ports.dedup();
    ports
}

/// /sys/class/tty/NAME/device is the USB interface of a USB serial port;
/// its parent directory is the device, with idVendor and idProduct.
fn linux_ports(id: UsbId) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir("/sys/class/tty") else {
        return vec![];
    };
    let hex = |path: PathBuf| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| u16::from_str_radix(text.trim(), 16).ok())
    };
    entries
        .flatten()
        .filter(|entry| {
            let device = entry.path().join("device/..");
            hex(device.join("idVendor")) == Some(id.vendor)
                && hex(device.join("idProduct")) == Some(id.product)
        })
        .map(|entry| Path::new("/dev").join(entry.file_name()))
        .collect()
}

/// The call-out devices (/dev/cu.*) in `ioreg -l` output that belong to a
/// USB device with `id`: each IOSerialBSDClient's nearest ancestor with
/// USB IDs is its device.
pub fn ioreg_ports(text: &str, id: UsbId) -> Vec<PathBuf> {
    // Open nodes: the column of their "+-o", and their IDs so far.
    let mut nodes: Vec<(usize, Option<u16>, Option<u16>)> = vec![];
    let mut ports = vec![];
    for line in text.lines() {
        if let Some(column) = line.find("+-o ") {
            while nodes.last().is_some_and(|&(open, _, _)| open >= column) {
                nodes.pop();
            }
            nodes.push((column, None, None));
            continue;
        }
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        let key = key.trim_start_matches([' ', '|']).trim_matches('"');
        let Some(node) = nodes.last_mut() else {
            continue;
        };
        match key {
            "idVendor" => node.1 = value.trim().parse().ok(),
            "idProduct" => node.2 = value.trim().parse().ok(),
            "IOCalloutDevice" => {
                let device = nodes
                    .iter()
                    .rev()
                    .find_map(|&(_, vendor, product)| Some((vendor?, product?)));
                if device == Some((id.vendor, id.product)) {
                    ports.push(PathBuf::from(value.trim().trim_matches('"')));
                }
            }
            _ => {}
        }
    }
    ports
}

/// Open `port`, set it to [`BOOTLOADER_BAUD`], and close it: the "1200
/// baud touch" that restarts the firmware into its bootloader.
#[cfg(all(unix, feature = "llvm"))]
pub fn request_bootloader(port: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(port.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let failed = |action: &str| {
        let error = std::io::Error::last_os_error();
        let hint =
            if error.kind() == std::io::ErrorKind::PermissionDenied && cfg!(target_os = "linux") {
                "; add yourself to the group that owns it (usually dialout) and log in again"
            } else {
                ""
            };
        format!("cannot {action} {}: {error}{hint}", port.display())
    };
    // SAFETY: plain POSIX calls on a descriptor this function owns, with a
    // termios structure that tcgetattr fills before it is used.
    unsafe {
        let fd = libc::open(
            path.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK,
        );
        if fd < 0 {
            return Err(failed("open"));
        }
        let mut settings: libc::termios = std::mem::zeroed();
        let result = if libc::tcgetattr(fd, &mut settings) != 0 {
            Err(failed("read the settings of"))
        } else if libc::cfsetispeed(&mut settings, libc::B1200) != 0
            || libc::cfsetospeed(&mut settings, libc::B1200) != 0
            || libc::tcsetattr(fd, libc::TCSANOW, &settings) != 0
        {
            Err(failed("set the baud rate of"))
        } else {
            Ok(())
        };
        libc::close(fd);
        result
    }
}

/// Without POSIX terminals the port cannot be set from here.
#[cfg(not(all(unix, feature = "llvm")))]
pub fn request_bootloader(port: &Path) -> Result<(), String> {
    Err(format!(
        "cannot set {} to {BOOTLOADER_BAUD} baud on this system",
        port.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IOREG: &str = r#"+-o Debug Probe@01100000  <class IOUSBHostDevice, id 0x1, registered>
  | {
  |   "idVendor" = 11914
  |   "idProduct" = 12
  | }
  | 
  +-o IOUSBHostInterface@1  <class IOUSBHostInterface, id 0x2>
  | | {
  | |   "idProduct" = 12
  | |   "idVendor" = 11914
  | | }
  | | 
  | +-o AppleUSBACMData  <class AppleUSBACMData, id 0x3>
  |   +-o IOSerialBSDClient  <class IOSerialBSDClient, id 0x4>
  |         "IOCalloutDevice" = "/dev/cu.usbmodem1101"
  |         "IODialinDevice" = "/dev/tty.usbmodem1101"
+-o USB Hub@02000000  <class IOUSBHostDevice, id 0x5>
  | {
  |   "idVendor" = 1452
  |   "idProduct" = 4136
  | }
  +-o Pico@02100000  <class IOUSBHostDevice, id 0x6>
    | {
    |   "idVendor" = 11914
    |   "idProduct" = 10
    | }
    +-o IOUSBHostInterface@1  <class IOUSBHostInterface, id 0x7>
      +-o AppleUSBACMData  <class AppleUSBACMData, id 0x8>
        +-o IOSerialBSDClient  <class IOSerialBSDClient, id 0x9>
              "IOCalloutDevice" = "/dev/cu.usbmodem2101"
"#;

    #[test]
    fn ioreg_ports_belong_to_the_nearest_device_with_ids() {
        let pico = UsbId {
            vendor: 0x2E8A,
            product: 0x000A,
        };
        assert_eq!(
            ioreg_ports(IOREG, pico),
            [PathBuf::from("/dev/cu.usbmodem2101")]
        );
        let probe = UsbId {
            vendor: 0x2E8A,
            product: 0x000C,
        };
        assert_eq!(
            ioreg_ports(IOREG, probe),
            [PathBuf::from("/dev/cu.usbmodem1101")]
        );
        let hub = UsbId {
            vendor: 1452,
            product: 4136,
        };
        assert!(ioreg_ports(IOREG, hub).is_empty());
    }
}
