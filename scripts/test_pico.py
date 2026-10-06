#!/usr/bin/env python3
"""Run a board self-test on a connected Raspberry Pi Pico or Pico 2.

The board must be in BOOTSEL mode (the RPI-RP2 or RP2350 drive is mounted).
The test firmware always reboots into BOOTSEL when it finishes, panics, or
hangs, so this script can be run repeatedly without touching the board.

Requires ld.lld and picotool. Usage:

    python3 scripts/test_pico.py [--board pico|pico2] [--dodo target/debug/dodo] [-O LEVEL ...]
"""
import argparse
import os
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Per board: the bootloader drive's volume name and where picotool reads the
# self-test's result record. The RP2040 boot ROM keeps SRAM, so the record
# stays at RECORD in tests/hardware/pico/main.dodo; the RP2350 boot ROM
# clears SRAM, so tests/hardware/pico2/main.dodo writes it to the last flash
# sector.
BOARDS = {
    "pico": ("RPI-RP2", 0x2003F000),
    "pico2": ("RP2350", 0x103FF000),
}
CHECK_NAMES = [
    "second take() is refused",
    ".data initialized from flash",
    "clk_sys is at full speed",
    "timer agrees with clk_sys",
    "10 ms delay lasts 10 ms",
]
LAST_CHECKS = [
    "invalid crystal is rejected",
    "chip take() after board take() is refused",
    "delay_ms(10) lasts 10 ms",
    "board NAME",
]


def wait_for_drive(drives, seconds):
    deadline = time.monotonic() + seconds
    while True:
        for drive in drives:
            if (drive / "INFO_UF2.TXT").exists():
                return drive
        if time.monotonic() >= deadline:
            return None
        time.sleep(0.25)


def candidate_drives(volume):
    roots = [Path("/Volumes") / volume]
    user = os.environ.get("USER", "")
    roots += [Path("/media") / user / volume, Path("/run/media") / user / volume]
    roots += [Path(f"{letter}:/") for letter in "DEFGHIJKLMNOPQRSTUVWXYZ"] if os.name == "nt" else []
    return roots


def read_record(picotool, record):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "record.bin"
        subprocess.run(
            [picotool, "save", "-r", f"{record:x}", f"{record + 0x100:x}", str(path)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        return struct.unpack("<6I", path.read_bytes()[:24])


def run(dodo, picotool, board, level):
    volume, record = BOARDS[board]
    project = ROOT / "tests/hardware" / board
    if record < 0x20000000:
        # A flash record: erase the previous run's so it cannot be misread.
        subprocess.run(
            [picotool, "erase", "-r", f"{record:x}", f"{record + 0x1000:x}"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
    subprocess.run([dodo, "run", str(project), "-O", level, "-q"], check=True)
    # Let the board leave the bootloader before waiting for it to return.
    time.sleep(1)
    if wait_for_drive(candidate_drives(volume), 20) is None:
        print(f"-O{level}: the board did not return to BOOTSEL; check the LED and USB cable")
        return False
    state, detail, khz, delay_us, cycles, more = read_record(picotool, record)
    kind, value = state >> 16, state & 0xFFFF
    if kind == 0xDEAD:
        source = (project / "main.dodo").read_text().splitlines()[value - 1].strip()
        print(f"-O{level}: panicked at main.dodo:{value}:{detail}: {source}")
        return False
    if kind == 0x5747:
        print(f"-O{level}: hung or faulted after check {value}")
        return False
    if kind != 0x600D:
        print(f"-O{level}: no result record (read {state:#010x})")
        return False
    failed = [index for index in range(64) if (detail | more << 32) >> index & 1]
    print(
        f"-O{level}: {value - len(failed)}/{value} checks passed; "
        f"clk_sys {khz} kHz, 10 ms delay {delay_us} us, {cycles} cycles"
    )
    for index in failed:
        if index < len(CHECK_NAMES):
            name = CHECK_NAMES[index]
        elif index >= value - len(LAST_CHECKS):
            name = LAST_CHECKS[index - (value - len(LAST_CHECKS))]
        else:
            name = "arithmetic helper"
        print(f"  check {index} failed: {name}")
    return not failed


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--board", choices=sorted(BOARDS), default="pico")
    parser.add_argument("--dodo", default=str(ROOT / "target/debug/dodo"))
    parser.add_argument("--picotool", default="picotool")
    parser.add_argument("-O", dest="levels", action="append", default=[])
    args = parser.parse_args()
    volume = BOARDS[args.board][0]
    if wait_for_drive(candidate_drives(volume), 0) is None:
        sys.exit(f"no {volume} drive; hold BOOTSEL while connecting the board")
    results = [
        run(args.dodo, args.picotool, args.board, level) for level in args.levels or ["0", "2"]
    ]
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
