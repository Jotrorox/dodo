#!/usr/bin/env python3
"""Run the board self-test on a connected Espressif ESP32-C6-DevKitC-1.

Connect the board over USB, either connector. `dodo run` flashes the
self-test through the chip's boot ROM, and the self-test prints its result
on the chip's own USB serial port, which this script reads. Nothing needs
to be wired to the board, and no button needs to be pressed.

Requires ld.lld. Usage:

    python3 scripts/test_esp32c6.py [--dodo target/debug/dodo] [-O LEVEL ...] [--port PORT]
                                    [--project DIR]

`--project` runs another firmware project that follows the same protocol;
its checks are then named by number only. `--follow-resets` lets such a
project reset the chip on purpose, to check what survives a reset: after it
prints `selftest restarting`, the script waits for the USB serial port to
disappear and come back, sends another byte, and keeps reading, until
`selftest done N`. A panic report after `selftest restarting` is then
expected too; only one in the last part fails the run. The script holds an exclusive
lock on the board (DODO_ESP32C6_LOCK, default /tmp/dodo-esp32c6.lock) from
flashing until the result is read, so several runs, from several checkouts,
take turns instead of flashing over each other.

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


def run(dodo, optimization, port, project=SELFTEST, follow=False):
    lock = open(os.environ.get("DODO_ESP32C6_LOCK", "/tmp/dodo-esp32c6.lock"), "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        return run_locked(dodo, optimization, port, project, follow)
    finally:
        lock.close()


def current_boot(output):
    """The output since the last `selftest restarting`, and whether the chip
    announced one."""
    marker = output.rfind(b"selftest restarting")
    if marker < 0:
        return output, False
    return output[marker:], True


def unexpected_panic(output, follow):
    """A panic report that ends the run: any, or when following resets, one
    after the last boot's `selftest ready`, as a panic right after
    `selftest restarting` is announced."""
    if not follow:
        return b"dodo: " in output
    boot, restarting = current_boot(output)
    if not restarting:
        return b"dodo: " in boot
    ready = boot.find(b"selftest ready")
    return ready >= 0 and b"dodo: " in boot[ready:]


def read_part(fd, follow):
    """Read the output until `selftest done`, a panic report, or, when
    following resets, the port going away after `selftest restarting`. A
    reset that keeps the port, as a watchdog reset does, needs no new port:
    another byte goes to the new boot. Returns the output and whether the
    port went away for a restart."""
    output = b""
    deadline = time.time() + 30
    last_poke = 0.0
    while True:
        boot, restarting = current_boot(output) if follow else (output, False)
        if b"selftest done" in boot or unexpected_panic(output, follow):
            break
        if time.time() > deadline:
            break
        if b"selftest ready" not in boot and time.time() - last_poke > 0.5:
            try:
                os.write(fd, b"s")
            except OSError:
                if restarting:
                    return output, True
                raise
            last_poke = time.time()
        ready, _, _ = select.select([fd], [], [], 0.2)
        if ready:
            try:
                data = os.read(fd, 4096)
            except BlockingIOError:
                continue
            except OSError:
                if restarting:
                    return output, True
                # DODO_ESP32C6_KEEP_OUTPUT=1: report what arrived before
                # the board left the USB bus (it reset) instead of failing.
                if os.environ.get("DODO_ESP32C6_KEEP_OUTPUT"):
                    return output + b"\n(board disconnected)\n", False
                raise
            if not data and restarting:
                return output, True
            if follow and b"selftest restarting" in data:
                deadline = time.time() + 30
            output += data
    # A panic report ends with its location; collect the rest of it.
    end = time.time() + 0.5
    while time.time() < end:
        ready, _, _ = select.select([fd], [], [], 0.1)
        if ready:
            try:
                output += os.read(fd, 4096)
            except OSError:
                break
    return output, False


def open_retrying(port, seconds):
    deadline = time.time() + seconds
    while True:
        try:
            return open_port(port)
        except OSError:
            if time.time() > deadline:
                raise
            time.sleep(0.2)


def run_locked(dodo, optimization, port, project, follow=False):
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
    parts = []
    # The self-test waits for a byte before it starts printing.
    fd = open_retrying(port, 10)
    try:
        while True:
            output, restarted = read_part(fd, follow)
            parts.append(output.decode("utf-8", "replace"))
            if not restarted or len(parts) > 8:
                break
            os.close(fd)
            fd = None
            # The port disappears while the chip resets; wait for it again.
            time.sleep(0.5)
            fd = open_retrying(port or find_port(), 15)
    finally:
        if fd is not None:
            os.close(fd)
    return parts


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dodo", default=str(ROOT / "target/debug/dodo"))
    parser.add_argument("-O", dest="levels", action="append", default=[])
    parser.add_argument("--port", help="the board's serial port (default: found by USB ID)")
    parser.add_argument("--project", type=Path, default=SELFTEST,
                        help="the firmware project to run (default: the self-test)")
    parser.add_argument("--follow-resets", action="store_true",
                        help="keep reading across resets the project announces with `selftest restarting`")
    args = parser.parse_args()
    failed = False
    for level in args.levels or [""]:
        label = f"-O {level}" if level else "default optimization"
        print(f"== ESP32-C6-DevKitC-1 self-test, {label}")
        parts = run(args.dodo, level, args.port, args.project.resolve(), args.follow_resets)
        text = "".join(parts)
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
        last = parts[-1]
        if args.follow_resets:
            # Only a panic after the last boot's `selftest ready` counts.
            boot = last[last.rfind("selftest restarting") + 1:] if "selftest restarting" in last else last
            last = boot[boot.find("selftest ready"):] if "selftest ready" in boot else ""
        panic = re.search(r"dodo: .*", last)
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
