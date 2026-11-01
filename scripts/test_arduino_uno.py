#!/usr/bin/env python3
"""Run the Arduino Uno self-test (tests/hardware/arduino_uno) on a connected board.

Each of the three parts (core, wide, peripherals) is flashed with `dodo run`,
which writes it through the board's bootloader itself, and reports its
checks on the USB serial port at 115200 baud. Opening the port resets the
board, so each part runs from the start while this script listens. Nothing
needs to be wired to the board, and no tool besides dodo and ld.lld is used.

Usage:

    python3 scripts/test_arduino_uno.py [--dodo target/debug/dodo] [--port /dev/cu.usbmodem*] [-O LEVEL ...]

The port is found by its USB IDs unless --port or DODO_SERIAL_PORT names it.
"""
import argparse
import os
import re
import select
import subprocess
import sys
import termios
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROJECT = ROOT / "tests" / "hardware" / "arduino_uno"
PARTS = ["core", "wide", "peripherals"]


class Port:
    """The board's serial port at 115200 baud, 8N1, raw."""

    def __init__(self, path):
        self.fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        attrs = termios.tcgetattr(self.fd)
        attrs[0] = 0
        attrs[1] = 0
        attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
        attrs[3] = 0
        attrs[4] = attrs[5] = termios.B115200
        termios.tcsetattr(self.fd, termios.TCSANOW, attrs)
        self.text = ""

    def close(self):
        os.close(self.fd)

    def write(self, text):
        os.write(self.fd, text.encode())

    def wait_for(self, pattern, timeout):
        """Read until `pattern` matches the text since the last start line.
        Patterns end in a newline, so a line still arriving never matches."""
        end = time.time() + timeout
        while time.time() < end:
            ready, _, _ = select.select([self.fd], [], [], 0.05)
            if ready:
                try:
                    self.text += os.read(self.fd, 4096).decode("latin1").replace("\r", "")
                except BlockingIOError:
                    pass
            # The board restarts when the port opens; keep the last run.
            start = self.text.rfind("selftest ")
            run = self.text[start:] if start >= 0 else self.text
            match = re.search(pattern, run, re.M)
            if match:
                return match, run
        start = self.text.rfind("selftest ")
        return None, self.text[start:] if start >= 0 else self.text


def find_port(dodo):
    if os.environ.get("DODO_SERIAL_PORT"):
        return os.environ["DODO_SERIAL_PORT"]
    candidates = sorted(Path("/dev").glob("cu.usbmodem*")) + sorted(Path("/dev").glob("ttyACM*"))
    candidates += sorted(Path("/dev").glob("cu.usbserial*")) + sorted(Path("/dev").glob("ttyUSB*"))
    if len(candidates) == 1:
        return str(candidates[0])
    return None


def run_part(dodo, part, level, port_path):
    command = [dodo, "run", str(PROJECT), "-b", part]
    if level is not None:
        command += ["-O", str(level)]
    flash = subprocess.run(command, capture_output=True, text=True)
    sys.stdout.write(flash.stderr)
    if flash.returncode != 0:
        return False, [f"{part}: dodo run failed ({flash.returncode})"]
    # The program starts as soon as flashing ends; opening the port resets
    # the board into the bootloader, which starts it again a second later.
    port = Port(port_path)
    failures = []
    try:
        match, run = port.wait_for(r"^done (\d+)/(\d+)\n", 30)
        for line in run.splitlines():
            if line.startswith(("ok ", "FAIL ", "info ", "fmt ")):
                print(f"  {line}")
        if not match:
            failures.append(f"{part}: no result line; output:\n{run}")
            return False, failures
        failures += [f"{part}: {line}" for line in run.splitlines() if line.startswith("FAIL ")]
        if match.group(1) != match.group(2):
            failures.append(f"{part}: {match.group(1)} of {match.group(2)} checks passed")
        if part == "core":
            if "fmt -12345 4294967295 true" not in run:
                failures.append("core: formatted line differs")
            ready, run = port.wait_for(r"^echo ready\n", 5)
            if not ready:
                failures.append("core: no echo prompt")
            else:
                port.write("hello dodo\n")
                echoed, run = port.wait_for(r"^echo (?!ready\n)(.*)\n", 5)
                if not echoed or echoed.group(1) != "HELLO DODO":
                    failures.append(f"core: echo returned {echoed.group(1) if echoed else 'nothing'}")
                else:
                    print("  echo HELLO DODO")
            panic, run = port.wait_for(r"^dodo: (.*) check failed at (.*):(\d+):(\d+)\n", 5)
            if not panic or panic.group(1) != "arithmetic overflow" or not panic.group(2).endswith("core.dodo"):
                failures.append(f"core: no panic report; output:\n{run}")
            else:
                print(f"  panic reported: {panic.group(0)}")
        if part == "wide" and "fmt 18446744073709551615 -9223372036854775807" not in run:
            failures.append("wide: formatted line differs")
    finally:
        port.close()
    return not failures, failures


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dodo", default=str(ROOT / "target" / "debug" / "dodo"))
    parser.add_argument("--port", help="the board's serial port (default: DODO_SERIAL_PORT, or the one USB serial port)")
    parser.add_argument("-O", dest="levels", action="append", type=int, help="optimization level; repeatable (default: the board's)")
    parser.add_argument("--part", action="append", choices=PARTS, help="run only these parts")
    args = parser.parse_args()
    port = args.port or find_port(args.dodo)
    if not port:
        sys.exit("no single USB serial port found; pass --port or set DODO_SERIAL_PORT")
    os.environ["DODO_SERIAL_PORT"] = port
    failures = []
    for level in args.levels or [None]:
        for part in args.part or PARTS:
            label = part if level is None else f"{part} -O {level}"
            print(f"== {label}")
            ok, part_failures = run_part(args.dodo, part, level, port)
            failures += [f"[{label}] {failure}" for failure in part_failures]
            print(f"== {label}: {'passed' if ok else 'FAILED'}")
    if failures:
        print("\nFailures:")
        for failure in failures:
            print(f"  {failure}")
        sys.exit(1)
    print("\nAll Arduino Uno self-test parts passed.")


if __name__ == "__main__":
    main()
