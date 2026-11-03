//! Vendor control transfers to a USB device, which is all a bootloader such
//! as micronucleus needs: IOKit's device interface on macOS and usbfs on
//! Linux, without a USB library. Other systems report that they are not
//! supported.
//!
//! Every request goes to the device (not an interface or endpoint) and has
//! the vendor type, so no interface has to be claimed and no system driver
//! is in the way.
use std::time::Duration;

/// A USB device, found by its vendor and product IDs.
pub struct Device {
    inner: platform::Handle,
    /// bcdDevice: the device's release number, as binary-coded decimal.
    pub release: u16,
}

/// Why a transfer failed.
#[derive(Debug)]
pub enum Error {
    /// The device is gone: unplugged, or restarted and enumerated again.
    Gone,
    /// It did not answer in time, or refused the request.
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Gone => f.write_str("the device is no longer connected"),
            Error::Failed(message) => f.write_str(message),
        }
    }
}

impl Device {
    /// The first connected device with these IDs, if any. An Err means the
    /// system could not be asked, or the device cannot be opened.
    pub fn open(vendor: u16, product: u16) -> Result<Option<Device>, String> {
        platform::open(vendor, product)
    }

    /// Release the device, for example before looking for it again after it
    /// enumerated anew.
    pub fn close(self) {}

    /// Send a vendor request to the device with `data`, if any.
    pub fn control_out(
        &self,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        timeout: Duration,
    ) -> Result<(), Error> {
        let mut copy = data.to_vec();
        self.inner
            .control(0x40, request, value, index, &mut copy, timeout)
            .map(|_| ())
    }

    /// A vendor request whose answer fills `buffer`; returns the bytes the
    /// device sent.
    pub fn control_in(
        &self,
        request: u8,
        value: u16,
        index: u16,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> Result<usize, Error> {
        self.inner
            .control(0xC0, request, value, index, buffer, timeout)
    }
}

#[cfg(all(target_os = "macos", feature = "llvm"))]
mod platform {
    //! IOKit: the device's IOUSBDeviceInterface182, whose DeviceRequestTO
    //! sends a control request with a timeout. Control requests need no
    //! exclusive access, so the device is not opened.
    use super::{Device, Error};
    use std::ffi::{CString, c_char, c_void};
    use std::time::Duration;

    type CFTypeRef = *const c_void;
    type IOReturn = i32;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Uuid([u8; 16]);

    /// kIOUSBDeviceUserClientTypeID
    const DEVICE_USER_CLIENT: Uuid = Uuid([
        0x9D, 0xC7, 0xB7, 0x80, 0x9E, 0xC0, 0x11, 0xD4, 0xA5, 0x4F, 0x00, 0x0A, 0x27, 0x05, 0x28,
        0x61,
    ]);
    /// kIOCFPlugInInterfaceID
    const PLUGIN_INTERFACE: Uuid = Uuid([
        0xC2, 0x44, 0xE8, 0x58, 0x10, 0x9C, 0x11, 0xD4, 0x91, 0xD4, 0x00, 0x50, 0xE4, 0xC6, 0x42,
        0x6F,
    ]);
    /// kIOUSBDeviceInterfaceID182
    const DEVICE_INTERFACE_182: Uuid = Uuid([
        0x15, 0x2F, 0xC4, 0x96, 0x48, 0x91, 0x11, 0xD5, 0x9D, 0x52, 0x00, 0x0A, 0x27, 0x80, 0x1E,
        0x86,
    ]);

    const RETURN_NO_DEVICE: IOReturn = 0xE000_02C0_u32 as i32;
    const RETURN_NOT_RESPONDING: IOReturn = 0xE000_02ED_u32 as i32;
    const RETURN_NOT_ATTACHED: IOReturn = 0xE000_02D9_u32 as i32;
    const RETURN_ABORTED: IOReturn = 0xE000_02EB_u32 as i32;
    const USB_TRANSACTION_TIMEOUT: IOReturn = 0xE000_4051_u32 as i32;
    const USB_PIPE_STALLED: IOReturn = 0xE000_404F_u32 as i32;

    const NUMBER_SINT32: isize = 3;
    const STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    /// IUnknown's functions, which every COM-style IOKit interface starts
    /// with.
    #[repr(C)]
    struct PlugInVtable {
        _reserved: *const c_void,
        query_interface: unsafe extern "C" fn(*mut c_void, Uuid, *mut *mut c_void) -> i32,
        _add_ref: unsafe extern "C" fn(*mut c_void) -> u32,
        release: unsafe extern "C" fn(*mut c_void) -> u32,
    }

    #[repr(C)]
    struct DeviceRequestTo {
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        data: *mut c_void,
        length_done: u32,
        no_data_timeout: u32,
        completion_timeout: u32,
    }

    type Unused = *const c_void;

    /// IOUSBDeviceInterface182, up to DeviceRequestTO.
    #[repr(C)]
    struct DeviceVtable {
        _reserved: *const c_void,
        _query_interface: Unused,
        _add_ref: Unused,
        release: unsafe extern "C" fn(*mut c_void) -> u32,
        _create_device_async_event_source: Unused,
        _get_device_async_event_source: Unused,
        _create_device_async_port: Unused,
        _get_device_async_port: Unused,
        _usb_device_open: Unused,
        _usb_device_close: Unused,
        _get_device_class: Unused,
        _get_device_sub_class: Unused,
        _get_device_protocol: Unused,
        _get_device_vendor: Unused,
        _get_device_product: Unused,
        get_device_release_number: unsafe extern "C" fn(*mut c_void, *mut u16) -> IOReturn,
        _get_device_address: Unused,
        _get_device_bus_power_available: Unused,
        _get_device_speed: Unused,
        _get_number_of_configurations: Unused,
        _get_location_id: Unused,
        _get_configuration_descriptor_ptr: Unused,
        _get_configuration: Unused,
        _set_configuration: Unused,
        _get_bus_frame_number: Unused,
        _reset_device: Unused,
        _device_request: Unused,
        _device_request_async: Unused,
        _create_interface_iterator: Unused,
        _usb_device_open_seize: Unused,
        device_request_to: unsafe extern "C" fn(*mut c_void, *mut DeviceRequestTo) -> IOReturn,
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingServices(
            main: u32,
            matching: *mut c_void,
            iterator: *mut u32,
        ) -> i32;
        fn IOIteratorNext(iterator: u32) -> u32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IOCreatePlugInInterfaceForService(
            service: u32,
            plugin_type: CFTypeRef,
            interface_type: CFTypeRef,
            plugin: *mut *mut *const PlugInVtable,
            score: *mut i32,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFUUIDCreateFromUUIDBytes(allocator: CFTypeRef, bytes: Uuid) -> CFTypeRef;
        fn CFNumberCreate(allocator: CFTypeRef, kind: isize, value: *const c_void) -> CFTypeRef;
        fn CFStringCreateWithCString(
            allocator: CFTypeRef,
            text: *const c_char,
            encoding: u32,
        ) -> CFTypeRef;
        fn CFDictionarySetValue(dictionary: *mut c_void, key: CFTypeRef, value: CFTypeRef);
        fn CFRelease(object: CFTypeRef);
    }

    pub struct Handle {
        device: *mut *const DeviceVtable,
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: `device` is the interface QueryInterface returned, and
            // released only here.
            unsafe {
                ((**self.device).release)(self.device as *mut c_void);
            }
        }
    }

    /// Set `key` of a matching dictionary to an integer.
    unsafe fn match_number(dictionary: *mut c_void, key: &str, value: i32) {
        let key = CString::new(key).unwrap();
        // SAFETY: the dictionary is a mutable CF dictionary; the key and
        // number are created here and released once the dictionary holds
        // them.
        unsafe {
            let name =
                CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), STRING_ENCODING_UTF8);
            let number = CFNumberCreate(
                std::ptr::null(),
                NUMBER_SINT32,
                &value as *const i32 as *const c_void,
            );
            CFDictionarySetValue(dictionary, name, number);
            CFRelease(name);
            CFRelease(number);
        }
    }

    pub fn open(vendor: u16, product: u16) -> Result<Option<Device>, String> {
        let class = CString::new("IOUSBHostDevice").unwrap();
        let mut iterator = 0u32;
        // SAFETY: IOKit calls with the arguments its headers describe; the
        // matching dictionary is consumed by IOServiceGetMatchingServices,
        // and every object obtained is released.
        unsafe {
            let matching = IOServiceMatching(class.as_ptr());
            if matching.is_null() {
                return Err("IOKit cannot match USB devices".into());
            }
            match_number(matching, "idVendor", vendor as i32);
            match_number(matching, "idProduct", product as i32);
            if IOServiceGetMatchingServices(0, matching, &mut iterator) != 0 {
                return Err("IOKit cannot list USB devices".into());
            }
            let mut found = None;
            let mut failure = None;
            loop {
                let service = IOIteratorNext(iterator);
                if service == 0 {
                    break;
                }
                if found.is_none() {
                    match interface(service) {
                        Ok(device) => found = Some(device),
                        Err(error) => failure = Some(error),
                    }
                }
                IOObjectRelease(service);
            }
            IOObjectRelease(iterator);
            match (found, failure) {
                (Some(device), _) => Ok(Some(device)),
                (None, Some(error)) => Err(error),
                (None, None) => Ok(None),
            }
        }
    }

    /// The device interface of a USB device service.
    unsafe fn interface(service: u32) -> Result<Device, String> {
        // SAFETY: see `open`.
        unsafe {
            let plugin_type = CFUUIDCreateFromUUIDBytes(std::ptr::null(), DEVICE_USER_CLIENT);
            let interface_type = CFUUIDCreateFromUUIDBytes(std::ptr::null(), PLUGIN_INTERFACE);
            let mut plugin: *mut *const PlugInVtable = std::ptr::null_mut();
            let mut score = 0i32;
            let status = IOCreatePlugInInterfaceForService(
                service,
                plugin_type,
                interface_type,
                &mut plugin,
                &mut score,
            );
            CFRelease(plugin_type);
            CFRelease(interface_type);
            if status != 0 || plugin.is_null() {
                return Err(format!(
                    "cannot open the USB device (IOKit error {status:#010x})"
                ));
            }
            let mut device: *mut c_void = std::ptr::null_mut();
            let result = ((**plugin).query_interface)(
                plugin as *mut c_void,
                DEVICE_INTERFACE_182,
                &mut device,
            );
            ((**plugin).release)(plugin as *mut c_void);
            if result != 0 || device.is_null() {
                return Err(format!(
                    "the USB device has no device interface (error {result:#010x})"
                ));
            }
            let device = device as *mut *const DeviceVtable;
            let mut release = 0u16;
            ((**device).get_device_release_number)(device as *mut c_void, &mut release);
            Ok(Device {
                inner: Handle { device },
                release,
            })
        }
    }

    impl Handle {
        pub fn control(
            &self,
            request_type: u8,
            request: u8,
            value: u16,
            index: u16,
            buffer: &mut [u8],
            timeout: Duration,
        ) -> Result<usize, Error> {
            let milliseconds = timeout.as_millis().clamp(1, u32::MAX as u128) as u32;
            let mut transfer = DeviceRequestTo {
                request_type,
                request,
                value,
                index,
                length: buffer.len() as u16,
                data: if buffer.is_empty() {
                    std::ptr::null_mut()
                } else {
                    buffer.as_mut_ptr() as *mut c_void
                },
                length_done: 0,
                no_data_timeout: milliseconds,
                completion_timeout: milliseconds,
            };
            // SAFETY: the request points at `buffer`, which outlives the
            // synchronous call, with its length.
            let status = unsafe {
                ((**self.device).device_request_to)(self.device as *mut c_void, &mut transfer)
            };
            match status {
                0 => Ok(transfer.length_done as usize),
                RETURN_NO_DEVICE | RETURN_NOT_ATTACHED | RETURN_ABORTED => Err(Error::Gone),
                RETURN_NOT_RESPONDING | USB_TRANSACTION_TIMEOUT => {
                    Err(Error::Failed("the device did not answer".into()))
                }
                USB_PIPE_STALLED => Err(Error::Failed("the device refused the request".into())),
                other => Err(Error::Failed(format!(
                    "USB request failed (IOKit error {other:#010x})"
                ))),
            }
        }
    }
}

#[cfg(all(target_os = "linux", feature = "llvm"))]
mod platform {
    //! usbfs: the device node /dev/bus/usb/BUS/DEVICE that sysfs names,
    //! and its USBDEVFS_CONTROL ioctl. Users need write access to the node,
    //! which a udev rule grants (see the hardware documentation).
    use super::{Device, Error};
    use std::fs::{self, File, OpenOptions};
    use std::os::fd::AsRawFd;
    use std::path::Path;
    use std::time::Duration;

    #[repr(C)]
    struct ControlTransfer {
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        timeout: u32,
        data: *mut libc::c_void,
    }

    /// _IOWR('U', 0, struct usbdevfs_ctrltransfer)
    const USBDEVFS_CONTROL: libc::c_ulong = (3 << 30)
        | ((std::mem::size_of::<ControlTransfer>() as libc::c_ulong) << 16)
        | ((b'U' as libc::c_ulong) << 8);

    pub struct Handle {
        file: File,
    }

    fn hex_file(path: &Path) -> Option<u16> {
        u16::from_str_radix(fs::read_to_string(path).ok()?.trim(), 16).ok()
    }

    fn decimal_file(path: &Path) -> Option<u32> {
        fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    pub fn open(vendor: u16, product: u16) -> Result<Option<Device>, String> {
        let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") else {
            return Ok(None);
        };
        let mut devices: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        devices.sort();
        for device in devices {
            if hex_file(&device.join("idVendor")) != Some(vendor)
                || hex_file(&device.join("idProduct")) != Some(product)
            {
                continue;
            }
            let (Some(bus), Some(number)) = (
                decimal_file(&device.join("busnum")),
                decimal_file(&device.join("devnum")),
            ) else {
                continue;
            };
            let node = format!("/dev/bus/usb/{bus:03}/{number:03}");
            let file = OpenOptions::new().read(true).write(true).open(&node).map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!(
                        "no permission to open {node}; add a udev rule for the bootloader (see the hardware documentation) or run as root"
                    )
                } else {
                    format!("cannot open {node}: {e}")
                }
            })?;
            return Ok(Some(Device {
                inner: Handle { file },
                release: hex_file(&device.join("bcdDevice")).unwrap_or(0),
            }));
        }
        Ok(None)
    }

    impl Handle {
        pub fn control(
            &self,
            request_type: u8,
            request: u8,
            value: u16,
            index: u16,
            buffer: &mut [u8],
            timeout: Duration,
        ) -> Result<usize, Error> {
            let mut transfer = ControlTransfer {
                request_type,
                request,
                value,
                index,
                length: buffer.len() as u16,
                timeout: timeout.as_millis().clamp(1, u32::MAX as u128) as u32,
                data: buffer.as_mut_ptr() as *mut libc::c_void,
            };
            // SAFETY: the ioctl reads the transfer and reads or writes
            // `buffer`, which outlives the synchronous call, up to its
            // length.
            let result =
                unsafe { libc::ioctl(self.file.as_raw_fd(), USBDEVFS_CONTROL as _, &mut transfer) };
            if result >= 0 {
                return Ok(result as usize);
            }
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::ENODEV) | Some(libc::ESHUTDOWN) => Err(Error::Gone),
                Some(libc::ETIMEDOUT) => Err(Error::Failed("the device did not answer".into())),
                Some(libc::EPIPE) => Err(Error::Failed("the device refused the request".into())),
                _ => Err(Error::Failed(format!("USB request failed: {error}"))),
            }
        }
    }
}

#[cfg(not(any(
    all(target_os = "macos", feature = "llvm"),
    all(target_os = "linux", feature = "llvm")
)))]
mod platform {
    use super::{Device, Error};
    use std::time::Duration;

    pub struct Handle;

    pub fn open(_vendor: u16, _product: u16) -> Result<Option<Device>, String> {
        Err("talking to USB bootloaders is supported on macOS and Linux only".into())
    }

    impl Handle {
        pub fn control(
            &self,
            _request_type: u8,
            _request: u8,
            _value: u16,
            _index: u16,
            _buffer: &mut [u8],
            _timeout: Duration,
        ) -> Result<usize, Error> {
            Err(Error::Gone)
        }
    }
}
