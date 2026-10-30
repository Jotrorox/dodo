#!/usr/bin/env python3
"""Check the RP2350's coprocessor binary64 helpers on a connected Pico 2 or Pico 2 W.

Flashes tests/hardware/rp2350_float, which compares every helper in
src/hardware/rp2350/dcp.ll with the software helper it replaces, on
pseudo-random operands and under interrupts, then times both. The board must
be in BOOTSEL mode (the RP2350 drive is mounted); the test always reboots
into BOOTSEL when it finishes, panics, or hangs.

Requires ld.lld and picotool. Usage:

    python3 scripts/test_rp2350_float.py [--board pico2|pico2_w] [--dodo target/debug/dodo] [-O LEVEL ...]
"""
import argparse
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from test_pico import ROOT, candidate_drives, wait_for_drive

RECORD = 0x103FF000
HELPERS = ["dadd", "dsub", "dmul", "ddiv", "i2d", "ui2d", "d2iz", "d2uiz", "d2f"]


def read_record(picotool):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "record.bin"
        subprocess.run(
            [picotool, "save", "-r", f"{RECORD:x}", f"{RECORD + 0x100:x}", str(path)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        data = path.read_bytes()
    return struct.unpack("<64I", data[:256]), data[192:256].split(b"\0")[0].decode(errors="replace")


def run(dodo, picotool, board, level):
    project = ROOT / "tests/hardware/rp2350_float"
    subprocess.run(
        [picotool, "erase", "-r", f"{RECORD:x}", f"{RECORD + 0x1000:x}"],
        check=True,
        stdout=subprocess.DEVNULL,
    )
    subprocess.run([dodo, "run", str(project), "-b", board, "-O", level, "-q"], check=True)
    time.sleep(1)
    if wait_for_drive(candidate_drives("RP2350"), 600) is None:
        print(f"-O{level}: the board did not return to BOOTSEL; check the LED and USB cable")
        return False
    words, file = read_record(picotool)
    kind, value = words[0] >> 16, words[0] & 0xFFFF
    if kind == 0xDEAD:
        source = (project / "main.dodo").read_text().splitlines()[value - 1].strip()
        where = f"...{file}" if file and not file.endswith("main.dodo") else "main.dodo"
        print(f"-O{level}: panicked at {where}:{value}:{words[1]}: {source}")
        return False
    if kind == 0x5747:
        print(f"-O{level}: hung or faulted while comparing {HELPERS[value]}")
        return False
    if kind != 0x600D:
        print(f"-O{level}: no result record (read {words[0]:#010x})")
        return False
    mismatches, cases, interrupts, interrupt_mismatches = words[1:5]
    print(
        f"-O{level}: {mismatches} mismatches in {value} x {cases} cases; "
        f"{interrupt_mismatches} in {interrupts} interrupts"
    )
    if mismatches:
        helper = words[5]
        a, b, got, want = (words[6 + 2 * i] | words[7 + 2 * i] << 32 for i in range(4))
        print(f"  first: {HELPERS[helper]}({a:#x}, {b:#x}) = {got:#x}, software {want:#x}")
    print("  cycles per call (software -> coprocessor): " + ", ".join(
        f"{name} {words[16 + 2 * i]} -> {words[17 + 2 * i]}" for i, name in enumerate(HELPERS)
    ))
    return mismatches == 0 and interrupt_mismatches == 0 and interrupts > 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--board", choices=["pico2", "pico2_w"], default="pico2")
    parser.add_argument("--dodo", default=str(ROOT / "target/debug/dodo"))
    parser.add_argument("--picotool", default="picotool")
    parser.add_argument("-O", dest="levels", action="append", default=[])
    args = parser.parse_args()
    if wait_for_drive(candidate_drives("RP2350"), 0) is None:
        sys.exit("no RP2350 drive; hold BOOTSEL while connecting the board")
    results = [run(args.dodo, args.picotool, args.board, level) for level in args.levels or ["0", "2"]]
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
