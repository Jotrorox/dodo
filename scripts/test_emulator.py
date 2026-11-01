#!/usr/bin/env python3
"""Run the firmware self-tests in tests/emulator on emulated RP2040 and RP2350 chips.

Builds the self-test for each board and optimization level with the Dodo
compiler and runs it in the emulator runner in emulator/, which boots the
firmware with the real boot ROM and reports the check that failed, the fault a
core took, or a hang. The runner wraps the rp2040-emu and rp2350-emu crates
with the fixes in emulator/patches. The crates and the boot ROMs are
downloaded once, checked against their SHA-256 sums, and kept in
emulator/target with the built runner, which is rebuilt only when emulator/
changes.

Requires ld.lld (to link the firmware), patch, and, to build the runner, cargo.
Usage:

    python3 scripts/test_emulator.py [--dodo target/debug/dodo] [--board pico|pico2 ...] [-O LEVEL ...]
"""
import argparse
import concurrent.futures
import hashlib
import os
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EMULATOR = ROOT / "emulator"
TARGET = EMULATOR / "target"
CACHE = TARGET / "cache"
PATCHED = TARGET / "patched"
RUNNER = TARGET / "release" / ("dodo-emulator.exe" if os.name == "nt" else "dodo-emulator")
FIRMWARE = TARGET / "firmware"

# Raspberry Pi's published boot ROMs (BSD-3-Clause).
BOOT_ROMS = {
    "rp2040": (
        "rp2040-bootrom-b2.elf",
        "https://github.com/raspberrypi/pico-bootrom-rp2040/releases/download/b2/b2.elf",
        "3a3c4c617d83bc9b910a6fa7bfdffa11ce26ff89370271d861507be821cc49aa",
    ),
    "rp2350": (
        "rp2350-bootrom-a4.bin",
        "https://github.com/raspberrypi/pico-bootrom-rp2350/releases/download/A4/bootrom-combined.bin",
        "c5f9c48c6539f043c423344a53ceaea3774dd1e4c337af49da741a5d259c84bc",
    ),
}
# The emulator crates patches/ applies to, as in emulator/Cargo.lock.
CRATES = {
    "rp2040-emu-0.1.5": "89f77488dd9c09e73a8e1425d2852ec031389f1a5afd2a402c2edb84751f4f7d",
    "rp2350-emu-0.2.6": "b5cffe3ca673f0c6b98d35e4b67414b23f616ae8b7a99ff8a1eebde1dca122fc",
}
BOARDS = {"pico": "rp2040", "pico2": "rp2350"}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def download(name, url, digest):
    """The cached file `name`, downloaded from `url` if missing or wrong."""
    path = CACHE / name
    if path.exists() and sha256(path) == digest:
        return path
    CACHE.mkdir(parents=True, exist_ok=True)
    for attempt in range(4):
        try:
            with urllib.request.urlopen(url, timeout=60) as response:
                data = response.read()
            break
        except OSError as error:
            if attempt == 3:
                raise SystemExit(f"cannot download {url}: {error}")
            time.sleep(2**attempt)
    actual = hashlib.sha256(data).hexdigest()
    if actual != digest:
        raise SystemExit(f"{url}: SHA-256 {actual}, expected {digest}")
    partial = path.with_suffix(".partial")
    partial.write_bytes(data)
    partial.replace(path)
    return path


def prepare_crates():
    """Unpack each emulator crate into target/patched with its patch applied."""
    for crate, digest in CRATES.items():
        patch = EMULATOR / "patches" / f"{crate}.patch"
        stamp = f"{digest} {sha256(patch)}\n"
        directory = PATCHED / crate
        if (directory / ".dodo-patched").exists() and (directory / ".dodo-patched").read_text() == stamp:
            continue
        name = crate.rsplit("-", 1)[0]
        archive = download(
            f"{crate}.crate", f"https://static.crates.io/crates/{name}/{crate}.crate", digest
        )
        shutil.rmtree(directory, ignore_errors=True)
        PATCHED.mkdir(parents=True, exist_ok=True)
        with tarfile.open(archive) as tar:
            members = tar.getmembers()
            for member in members:
                parts = Path(member.name).parts
                if not parts or parts[0] != crate or ".." in parts or not (member.isfile() or member.isdir()):
                    raise SystemExit(f"{archive}: unexpected entry {member.name}")
            tar.extractall(PATCHED, members=members)
        subprocess.run(["patch", "-p1", "-s", "-d", str(directory), "-i", str(patch)], check=True)
        (directory / ".dodo-patched").write_text(stamp)


def runner_key():
    """A digest of everything the runner is built from."""
    digest = hashlib.sha256()
    sources = [EMULATOR / "Cargo.toml", EMULATOR / "Cargo.lock"]
    sources += sorted((EMULATOR / "src").rglob("*.rs")) + sorted((EMULATOR / "patches").glob("*.patch"))
    for path in sources:
        digest.update(path.relative_to(EMULATOR).as_posix().encode() + b"\0" + path.read_bytes())
    return digest.hexdigest()


def build_runner():
    key = runner_key()
    stamp = RUNNER.with_name(RUNNER.name + ".key")
    if RUNNER.exists() and stamp.exists() and stamp.read_text() == key:
        return
    prepare_crates()
    subprocess.run(
        ["cargo", "build", "--release", "--locked", "--manifest-path", str(EMULATOR / "Cargo.toml")],
        check=True,
    )
    stamp.write_text(key)


def build_firmware(dodo, board, level):
    output = FIRMWARE / f"{board}-O{level}.elf"
    result = subprocess.run(
        [dodo, "build", str(ROOT / "tests/emulator"), "-b", board, "-O", level, "-o", str(output), "-q"],
        capture_output=True,
        text=True,
    )
    return output, result


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dodo", default=str(ROOT / "target/debug/dodo"))
    parser.add_argument("--board", action="append", choices=sorted(BOARDS), default=[])
    parser.add_argument("-O", dest="levels", action="append", choices=["0", "1", "2", "3"], default=[])
    args = parser.parse_args()
    boards = args.board or sorted(BOARDS)
    levels = args.levels or ["0", "2"]

    build_runner()
    roms = {chip: download(*BOOT_ROMS[chip]) for chip in {BOARDS[board] for board in boards}}
    FIRMWARE.mkdir(parents=True, exist_ok=True)
    jobs = [(board, level) for board in boards for level in levels]
    with concurrent.futures.ThreadPoolExecutor(max_workers=os.cpu_count() or 2) as pool:
        builds = list(pool.map(lambda job: build_firmware(args.dodo, *job), jobs))

    failed = []
    for (board, level), (firmware, build) in zip(jobs, builds):
        label = f"{board} -O{level}"
        if build.returncode != 0:
            print(f"{label}: build failed\n{build.stdout}{build.stderr}", end="")
            failed.append(label)
            continue
        rom = roms[BOARDS[board]]
        run = subprocess.run([str(RUNNER), "--bootrom", str(rom), str(firmware)], capture_output=True, text=True)
        print(f"{label}: {run.stdout.strip()}{run.stderr.strip()}", flush=True)
        if run.returncode != 0:
            failed.append(label)
    if failed:
        print(f"{len(failed)} of {len(jobs)} emulator runs failed: {', '.join(failed)}")
        return 1
    print(f"all {len(jobs)} emulator runs passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
