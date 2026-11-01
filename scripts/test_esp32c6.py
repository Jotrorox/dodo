#!/usr/bin/env python3
"""Run the board self-test on a connected Espressif ESP32-C6-DevKitC-1.

Connect the board over USB, either connector. `dodo run` flashes the
self-test through the chip's boot ROM, and the self-test prints its result
on the chip's own USB serial port, which this script reads. Nothing needs
to be wired to the board, and no button needs to be pressed.

Requires ld.lld. Usage:

    python3 scripts/test_esp32c6.py [--dodo target/debug/dodo] [-O LEVEL ...] [--port PORT]
                                    [--project DIR] [--reconnect]

`--project` runs another firmware project that follows the same protocol;
its checks are then named by number only. The script holds an exclusive
lock on the board (DODO_ESP32C6_LOCK, default /tmp/dodo-esp32c6.lock) from
flashing until the result is read, so several runs, from several checkouts,
take turns instead of flashing over each other. `--reconnect` is for a
project that releases the USB serial port for a while, such as across a
dormant state, which drops the connection: it prints `selftest waiting`
first and `selftest resumed` once back. The script then reopens the port
whenever it fails or disappears, or stays silent in between, and keeps
sending a byte while the board is silent, until `selftest done`.

The self-test (tests/hardware/esp32c6_devkitc/main.dodo) waits for this
script to send a byte, so no output is lost before the port is open, then
prints one line per check, `check N ok` or `check N FAILED`, and finally
`selftest done N` or a panic report. The script names each check from
CHECK_NAMES and exits nonzero if any failed.
"""
import argparse
import fcntl
import os
import re
import select
import subprocess
import sys
import termios
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SELFTEST = ROOT / "tests/hardware/esp32c6_devkitc"

CHECK_NAMES = [
    "second take() is refused",
    ".data initialized from flash",
    "CPU runs at 160 MHz",
    "10 ms delay lasts 10 ms",
    "pins are owned once and validated",
    "output pin reads back through the input path",
    "pull-up and pull-down are seen by an input",
    "GPIO edge is latched and cleared",
    "RGB LED takes colors and keeps its pin",
    "I2C pins and frequency are validated",
    "I2C runs at 100 kHz; controller and pins are owned once",
    "I2C read from an absent device is NoAcknowledge",
    "I2C address phase lasts about 100 us",
    "I2C probe, write, write_read, and 10-bit write to an absent device fail",
    "SPI pins and frequency are validated",
    "SPI loopback reads back 16 bytes; controller is owned once",
    "SPI clock is 1 MHz",
    "SPI mode 3 write and loopback, 16 bytes and 1",
    "SPI loopback of 200 bytes at 10 MHz",
    "released I2C pins clock by hand and stay pulled up",
    "released buses are claimed again and run",
    "UART pins and baud rate are validated",
    "UART at 115200 baud; both controllers and pins are owned once",
    "UART loopback reads back 16 bytes in 1.39 ms",
    "UART newlines go out as CR LF and read back as LF",
    "UART 8E2 at 1 Mbaud fills the receive FIFO, then reports one overrun",
    "released UART pins idle high; the port is claimed again",
    "UART bytes go out through the TX pad and come back",
    "PWM output is validated",
    "PWM outputs share a 1 kHz timer with 16384 steps; owned once",
    "PWM output is high 25 % of the time",
    "PWM output rises 10 times in 10 ms",
    "ADC inputs are validated",
    "ADC reads 3.3 V and ground through the pad's pulls; owned once",
    "temperature sensor reads 0 to 60 C",
    "timer alarms are claimed once and numbered 0 to 2",
    "alarm interrupt fires once, 2 ms after scheduling",
    "alarm sleeps until its deadline without taking its interrupt",
    "GPIO interrupt runs its handler",
    "software interrupt waits until enabled, then runs once",
    "masked software interrupt runs when the critical section ends",
    "executor runs tasks on the alarm clock",
    "function placed in SRAM runs from SRAM",
    "DMA copies 10000 bytes and reports no error",
    "DMA channels are owned once; paced transfers are refused",
    "u64 and i64 division, remainder, and multiplication",
    "f32 arithmetic and conversions",
    "f64 arithmetic and conversions",
    "atomic read-modify-write",
    "USB serial reports a connected host",
    "board constants name usable pins",
]


def open_port(path):
    fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    attrs[0] = 0  # iflag
    attrs[1] = 0  # oflag
    attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    attrs[3] = 0  # lflag
    attrs[4] = attrs[5] = termios.B115200
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    return fd


def find_port():
    """The chip's own USB serial port (Espressif 303A:1001)."""
    if sys.platform == "darwin":
        out = subprocess.run(
            ["ioreg", "-r", "-c", "IOUSBHostDevice", "-l", "-w0"],
            capture_output=True, text=True, check=False).stdout
        found = None
        ports = []
        for line in out.splitlines():
            if "+-o " in line:
                found = None if "IOUSBHostDevice" in line else found
            if '"idVendor" = 12346' in line:
                found = True
            match = re.search(r'"IOCalloutDevice" = "([^"]+)"', line)
            if match and found:
                ports.append(match.group(1))
        return ports[0] if ports else None
    for tty in sorted(Path("/sys/class/tty").glob("ttyACM*")):
        device = tty / "device/.."
        try:
            if (device / "idVendor").read_text().strip() == "303a":
                return f"/dev/{tty.name}"
        except OSError:
            pass
    return None


def run(dodo, optimization, port, project=SELFTEST, reconnect=False):
    lock = open(os.environ.get("DODO_ESP32C6_LOCK", "/tmp/dodo-esp32c6.lock"), "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        return run_locked(dodo, optimization, port, project, reconnect)
    finally:
        lock.close()


def set_lines(fd, dtr, rts):
    import array
    for line, on in sorted([(termios.TIOCM_DTR, dtr), (termios.TIOCM_RTS, rts)], key=lambda x: not x[1]):
        fcntl.ioctl(fd, termios.TIOCMBIS if on else termios.TIOCMBIC, array.array("i", [line]))


def slip(packet):
    return b"\xc0" + packet.replace(b"\xdb", b"\xdb\xdd").replace(b"\xc0", b"\xdb\xdc") + b"\xc0"


def rom_command(fd, command, data, timeout=8.0):
    """Send a boot ROM command and return its value word, or None."""
    packet = bytes([0, command]) + len(data).to_bytes(2, "little") + bytes(4) + data
    os.write(fd, slip(packet))
    buffer = b""
    end = time.time() + timeout
    while time.time() < end:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if ready:
            try:
                buffer += os.read(fd, 4096)
            except BlockingIOError:
                pass
        for frame in buffer.split(b"\xc0"):
            frame = frame.replace(b"\xdb\xdc", b"\xc0").replace(b"\xdb\xdd", b"\xdb")
            if len(frame) >= 8 and frame[0] == 1 and frame[1] == command:
                return int.from_bytes(frame[4:8], "little")
    return None


def rom_session(port, action):
    """Reset the chip into the boot ROM's download mode inside the lock and
    run `action(read_reg, write_reg)`."""
    lock = open(os.environ.get("DODO_ESP32C6_LOCK", "/tmp/dodo-esp32c6.lock"), "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        path = port or find_port()
        fd = open_port(path)
        try:
            for attempt in range(4):
                set_lines(fd, False, False)
                time.sleep(0.1)
                set_lines(fd, True, False)
                time.sleep(0.1)
                set_lines(fd, False, True)
                time.sleep(0.1)
                set_lines(fd, False, False)
                termios.tcflush(fd, termios.TCIOFLUSH)
                if any(rom_command(fd, 0x08, b"\x07\x07\x12\x20" + b"\x55" * 32, 0.3) is not None
                       for _ in range(5)):
                    break
            else:
                sys.exit("the chip did not enter its download mode")
            time.sleep(0.5)
            termios.tcflush(fd, termios.TCIOFLUSH)

            def read_reg(address):
                return rom_command(fd, 0x0A, address.to_bytes(4, "little"))

            def write_reg(address, value):
                data = b"".join(x.to_bytes(4, "little") for x in [address, value, 0xFFFFFFFF, 0])
                return rom_command(fd, 0x09, data) is not None

            action(read_reg, write_reg)
        finally:
            os.close(fd)
    finally:
        lock.close()


def peek(port, addresses):
    """Print words of memory or registers, read through the boot ROM; the
    reset into its download mode keeps SRAM in the LP domain."""
    def action(read_reg, write_reg):
        for address in addresses:
            value = read_reg(address)
            print(f"  {address:#010x}: {'no answer' if value is None else f'{value:#010x}'}")
    rom_session(port, action)


def recover(port):
    """Reset the whole chip, its always-on (LP) domain and PMU included,
    which a reset through the USB serial port leaves alone: enter the boot
    ROM's download mode and let the LP watchdog's first stage reset the
    chip (RTC reset) after about 20 ms. For a board whose PMU state a
    previous run left inconsistent, so that the boot ROM stops answering."""
    lock = open(os.environ.get("DODO_ESP32C6_LOCK", "/tmp/dodo-esp32c6.lock"), "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        path = port or find_port()
        fd = open_port(path)
        try:
            for attempt in range(4):
                set_lines(fd, False, False)
                time.sleep(0.1)
                set_lines(fd, True, False)
                time.sleep(0.1)
                set_lines(fd, False, True)
                time.sleep(0.1)
                set_lines(fd, False, False)
                termios.tcflush(fd, termios.TCIOFLUSH)
                synced = False
                for _ in range(5):
                    if rom_command(fd, 0x08, b"\x07\x07\x12\x20" + b"\x55" * 32, 0.3) is not None:
                        synced = True
                        break
                if synced:
                    break
            else:
                sys.exit("the chip did not enter its download mode")
            time.sleep(0.5)
            termios.tcflush(fd, termios.TCIOFLUSH)
            for name, address in [("PMU main state", 0x600B018C), ("PMU power state", 0x600B0190),
                                  ("PMU clock state", 0x600B0194), ("PMU int raw", 0x600B015C),
                                  ("LP_CLKRST clock config", 0x600B0400)]:
                value = rom_command(fd, 0x0A, address.to_bytes(4, "little"))
                print(f"  {name}: {'no answer' if value is None else hex(value)}")

            def write_reg(address, value):
                data = b"".join(x.to_bytes(4, "little") for x in [address, value, 0xFFFFFFFF, 0])
                return rom_command(fd, 0x09, data) is not None

            wdt = 0x600B1C00
            ok = write_reg(wdt + 0x18, 0x50D83AA1)  # WDTWPROTECT: unlock
            ok &= write_reg(wdt + 0x04, 3000)  # WDTCONFIG1: stage 0 timeout, slow clock cycles
            # WDTCONFIG0: enabled, stage 0 resets the whole chip (4), reset lengths 7.
            ok &= write_reg(wdt + 0x00, (1 << 31) | (4 << 28) | (7 << 16) | (7 << 13))
            ok &= write_reg(wdt + 0x14, 1 << 31)  # WDTFEED
            print("  LP watchdog armed" if ok else "  the boot ROM did not take the watchdog writes")
        finally:
            os.close(fd)
        time.sleep(2)
    finally:
        lock.close()


def close_quietly(fd):
    """Close the port without a reset request: the USB serial port resets
    the chip when RTS is on while DTR is off, so RTS goes off first."""
    try:
        set_lines(fd, True, False)
        set_lines(fd, False, False)
    except OSError:
        pass
    os.close(fd)


def reopen(port, deadline):
    """Open the board's port again once it is back, or None by `deadline`."""
    while time.time() < deadline:
        path = port or find_port()
        if path:
            try:
                return open_port(path)
            except OSError:
                pass
        time.sleep(0.2)
    return None


def run_locked(dodo, optimization, port, project, reconnect=False):
    env = dict(os.environ)
    if port:
        env["DODO_SERIAL_PORT"] = port
    command = [dodo, "run", str(project)]
    if optimization:
        command += ["-O", optimization]
    subprocess.run(command, check=True, cwd=project, env=env)
    port = port or find_port()
    if not port:
        sys.exit("no ESP32-C6 USB serial port found after flashing")
    deadline = time.time() + 10
    fd = None
    while fd is None:
        try:
            fd = open_port(port)
        except OSError:
            if time.time() > deadline:
                raise
            time.sleep(0.2)
    output = b""
    try:
        # The self-test waits for a byte before it starts printing.
        deadline = time.time() + (60 if reconnect else 30)
        last_poke = 0.0
        last_output = time.time()
        opened = time.time()
        waits = 0
        while b"selftest done" not in output and b"dodo: " not in output:
            if time.time() > deadline:
                break
            if fd is None:
                fd = reopen(port, deadline)
                if fd is None:
                    break
                opened = time.time()
            quiet = time.time() - last_output > 0.5
            if reconnect and output.count(b"selftest waiting") > waits:
                # The project releases its port and drops the connection:
                # let go of the port meanwhile, so no stale descriptor is
                # left, and open it again once the board is back.
                waits += 1
                close_quietly(fd)
                fd = None
                time.sleep(4)
                continue
            if (b"selftest ready" not in output or (reconnect and quiet)) and time.time() - last_poke > 0.5:
                try:
                    os.write(fd, b"s")
                except OSError:
                    if not reconnect:
                        raise
                    os.close(fd)
                    fd = None
                    continue
                last_poke = time.time()
            ready, _, _ = select.select([fd], [], [], 0.2)
            if ready:
                try:
                    data = os.read(fd, 4096)
                except BlockingIOError:
                    data = None
                except OSError:
                    if not reconnect:
                        raise
                    os.close(fd)
                    fd = None
                    continue
                if data:
                    output += data
                    last_output = time.time()
                elif data is not None and reconnect:
                    # End of file: the device went away.
                    os.close(fd)
                    fd = None
        # A panic report ends with its location; collect the rest of it.
        end = time.time() + 0.5
        while fd is not None and time.time() < end:
            ready, _, _ = select.select([fd], [], [], 0.1)
            if ready:
                try:
                    output += os.read(fd, 4096)
                except (BlockingIOError, OSError):
                    pass
    finally:
        if fd is not None:
            os.close(fd)
    return output.decode("utf-8", "replace")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dodo", default=str(ROOT / "target/debug/dodo"))
    parser.add_argument("-O", dest="levels", action="append", default=[])
    parser.add_argument("--port", help="the board's serial port (default: found by USB ID)")
    parser.add_argument("--project", type=Path, default=SELFTEST,
                        help="the firmware project to run (default: the self-test)")
    parser.add_argument("--reconnect", action="store_true",
                        help="reopen the port when the project drops the USB connection")
    parser.add_argument("--recover", action="store_true",
                        help="first reset the whole chip, LP domain and PMU included, through the LP watchdog")
    parser.add_argument("--peek", action="append", default=[], type=lambda text: int(text, 0),
                        help="print a word read through the boot ROM, such as an LP SRAM breadcrumb")
    args = parser.parse_args()
    if args.peek:
        print("== reading through the boot ROM")
        peek(args.port, args.peek)
        if not args.levels and not args.recover:
            return
    if args.recover:
        print("== resetting the whole chip")
        recover(args.port)
        if not args.levels:
            return
    failed = False
    for level in args.levels or [""]:
        label = f"-O {level}" if level else "default optimization"
        print(f"== ESP32-C6-DevKitC-1 self-test, {label}")
        text = run(args.dodo, level, args.port, args.project.resolve(), args.reconnect)
        names = CHECK_NAMES if args.project.resolve() == SELFTEST else []
        results = {}
        for match in re.finditer(r"check (\d+) (ok|FAILED)", text):
            results[int(match.group(1))] = match.group(2) == "ok"
        for number in sorted(results):
            name = names[number] if number < len(names) else f"check {number}"
            status = "ok" if results[number] else "FAILED"
            print(f"  {number:2} {status:6} {name}")
            failed |= not results[number]
        for line in text.splitlines():
            line = line.strip()
            if line and not re.match(r"check \d+ |selftest |dodo: ", line):
                print(f"     {line}")
        done = re.search(r"selftest done (\d+)", text)
        panic = re.search(r"dodo: .*", text)
        if panic:
            print(f"  panicked: {panic.group(0).strip()}")
            failed = True
        elif not done:
            print("  no result: the self-test hung; its output was:")
            print("    " + text.replace("\n", "\n    "))
            failed = True
        elif names and int(done.group(1)) != len(names):
            print(f"  ran {done.group(1)} checks, expected {len(CHECK_NAMES)}")
            failed = True
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
