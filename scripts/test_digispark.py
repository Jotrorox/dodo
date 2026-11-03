#!/usr/bin/env python3
"""Run the Digispark self-test (tests/hardware/digispark) on a connected board.

Each part (core, wide, float, peripherals, buses) is flashed with `dodo run`,
which writes it through the board's micronucleus bootloader itself. The
board cannot send data to the computer, so each part reports through the
bootloader instead: it starts it again 3 s + 2 s * N after it began, N being
the number of the first failed check (0 if all passed), and this script
measures when the bootloader enumerates again on USB. The bootloader then
starts the part again after its five seconds, and so on, which keeps the
board reachable for the next part without unplugging it.

The first part waits for the bootloader: plug the board in (or unplug it and
plug it in again) when `dodo run` asks. Nothing needs to be wired to the
board, and no tool besides dodo and ld.lld is used.

Usage:

    python3 scripts/test_digispark.py [--dodo target/debug/dodo] [--part core ...]
"""
import argparse
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROJECT = ROOT / "tests" / "hardware" / "digispark"
PARTS = ["core", "wide", "float", "peripherals", "buses"]
VENDOR, PRODUCT = 0x16D0, 0x0753
# From the end of `dodo run` to the bootloader's enumeration: the
# bootloader's start, its USB reset, and the computer noticing it.
LATENCY = 0.6


def bootloaders():
    """Identifiers of the enumerated micronucleus devices, which change
    whenever the bootloader starts again."""
    if sys.platform == "darwin":
        text = subprocess.run(["ioreg", "-p", "IOUSB", "-l", "-w0"], capture_output=True, text=True).stdout
        found = set()
        for entry in re.split(r"\n(?=[ |]*\+-o )", text):
            if re.search(rf'"idVendor" = {VENDOR}\b', entry) and re.search(rf'"idProduct" = {PRODUCT}\b', entry):
                session = re.search(r'"sessionID" = (\d+)', entry)
                found.add(session.group(1) if session else entry[:80])
        return found
    found = set()
    for device in Path("/sys/bus/usb/devices").glob("*"):
        try:
            vendor = int((device / "idVendor").read_text(), 16)
            product = int((device / "idProduct").read_text(), 16)
        except (OSError, ValueError):
            continue
        if (vendor, product) == (VENDOR, PRODUCT):
            found.add((device.name, (device / "devnum").read_text().strip()))
    return found


def run_part(dodo, part, level, longest, wait):
    command = [dodo, "run", str(PROJECT), "-b", part]
    if level is not None:
        command += ["-O", str(level)]
    # `dodo run` waits a minute for the bootloader; keep asking for `wait`.
    give_up = time.monotonic() + wait
    while True:
        flash = subprocess.run(command, stderr=subprocess.PIPE, text=True)
        started = time.monotonic()
        sys.stdout.write(flash.stderr)
        sys.stdout.flush()
        if flash.returncode == 0 or "no micronucleus bootloader" not in flash.stderr or started > give_up:
            break
    if flash.returncode != 0:
        return False, [f"{part}: dodo run failed ({flash.returncode})"]
    # The device listed now is the bootloader that just started the part.
    before = bootloaders()
    deadline = started + 3 + 2 * longest + 5
    while time.monotonic() < deadline:
        if bootloaders() - before:
            break
        time.sleep(0.05)
    else:
        return False, [f"{part}: the bootloader did not come back; the part hung or reset"]
    gap = time.monotonic() - started
    code = round((gap - 3 - LATENCY) / 2)
    print(f"  bootloader back after {gap:.2f} s", flush=True)
    if code <= 0:
        return True, []
    return False, [f"{part}: check {code} failed (see tests/hardware/digispark/{part}.dodo)"]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dodo", default=str(ROOT / "target" / "debug" / "dodo"))
    parser.add_argument("-O", dest="levels", action="append", type=int, help="optimization level; repeatable (default: the board's)")
    parser.add_argument("--part", action="append", choices=PARTS, help="run only these parts")
    parser.add_argument("--checks", type=int, default=20, help="the most checks in a part, which bounds the wait")
    parser.add_argument("--wait", type=int, default=600, help="seconds to wait for the board to be plugged in")
    args = parser.parse_args()
    failures = []
    for level in args.levels or [None]:
        for part in args.part or PARTS:
            label = part if level is None else f"{part} -O {level}"
            print(f"== {label}", flush=True)
            ok, part_failures = run_part(args.dodo, part, level, args.checks, args.wait)
            failures += [f"[{label}] {failure}" for failure in part_failures]
            print(f"== {label}: {'passed' if ok else 'FAILED'}", flush=True)
            if any("dodo run failed" in failure for failure in part_failures):
                break
    if failures:
        print("\nFailures:")
        for failure in failures:
            print(f"  {failure}")
        sys.exit(1)
    print("\nAll Digispark self-test parts passed.")


if __name__ == "__main__":
    main()
