#!/usr/bin/env python3
"""Test harness for Dodo's Lua 5.4 implementation (std/script/lua).

Subcommands:

  fetch    download, verify, extract and build the reference PUC Lua 5.4.9
           interpreter plus the official test suite (into target/lua-reference)
  suite    run the official Lua 5.4.9 test suite, one file per process
  diff     run tests/lua/*.lua under our interpreter and the reference, compare
  expect   regenerate tests/lua/*.expected from the reference interpreter
  check    compare our interpreter against the committed .expected files
  bench    time benchmarks/lua/*.lua under both interpreters

Run `python3 scripts/test_lua.py <command> --help` for options. Only the Python
standard library is used; the script stays compatible with Python 3.9.
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import os
import platform
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request
from pathlib import Path, PurePosixPath
from typing import Dict, List, NamedTuple, Optional, Sequence, Tuple

REPO = Path(__file__).resolve().parent.parent

LUA_VERSION = "5.4.9"
LUA_URL = "https://www.lua.org/ftp/lua-%s.tar.gz" % LUA_VERSION
LUA_SHA256 = "2335b6c582a52654f94612bf10d2f4672805d05329aa6568b1d8cd9e5c6fb8e6"
TESTS_URL = "https://www.lua.org/tests/lua-%s-tests.tar.gz" % LUA_VERSION
TESTS_SHA256 = "7d971845f545ffc09fbb3128a86b2c6524161c70d0fdf0154a16e8c00c343fca"

DEFAULT_CACHE = REPO / "target" / "lua-reference"
DEFAULT_LUA = REPO / "build" / "lua"
CORPUS_DIR = REPO / "tests" / "lua"
BENCH_DIR = REPO / "benchmarks" / "lua"

# Environment variables that change interpreter behaviour; always removed so
# runs are reproducible regardless of the caller's shell.
LUA_ENV_VARS = (
    "LUA_INIT", "LUA_INIT_5_4", "LUA_PATH", "LUA_PATH_5_4",
    "LUA_CPATH", "LUA_CPATH_5_4",
)
# Variables the corpus may read via os.getenv.
TEST_ENV = {"LUA_TEST_VAR": "hello from env", "LC_ALL": "C", "LANG": "C", "TZ": "UTC"}


# --------------------------------------------------------------------------
# small helpers


def die(msg: str) -> "None":
    print("error: " + msg, file=sys.stderr)
    sys.exit(2)


def rel(p: Path) -> str:
    try:
        return str(p.resolve().relative_to(REPO))
    except ValueError:
        return str(p)


def resolve_interp(value: Optional[str], cache: Path, default: Path) -> Path:
    """Resolve an interpreter argument. The literal value 'ref' means the
    reference build inside the cache directory."""
    if value is None:
        path = default
    elif value == "ref":
        path = cache / "bin" / "lua"
    else:
        path = Path(value)
        if not path.is_absolute():
            # Bare names such as "lua" are looked up on PATH.
            if os.sep not in value and not path.exists():
                found = shutil.which(value)
                if found:
                    return Path(found)
            path = (Path.cwd() / path)
    path = path.resolve()
    if not path.is_file():
        die("interpreter not found: %s" % path)
    return path


def run_env(extra: Optional[Dict[str, str]] = None) -> Dict[str, str]:
    env = dict(os.environ)
    for name in LUA_ENV_VARS:
        env.pop(name, None)
    env.update(TEST_ENV)
    if extra:
        env.update(extra)
    return env


class Result(NamedTuple):
    code: Optional[int]  # None means timeout
    stdout: str
    stderr: str
    seconds: float


def run(argv: Sequence[str], cwd: Path, timeout: float,
        stdin: Optional[bytes] = None, env: Optional[Dict[str, str]] = None) -> Result:
    start = time.perf_counter()
    posix = os.name == "posix"
    # Own process group, so a timeout also kills processes the test spawned
    # (os.execute, io.popen, main.lua's background interpreters).
    proc = subprocess.Popen(
        list(argv), cwd=str(cwd), env=run_env(env), start_new_session=posix,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    try:
        out, err = proc.communicate(stdin if stdin is not None else b"", timeout=timeout)
    except subprocess.TimeoutExpired:
        if posix:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        else:
            proc.kill()
        out, err = proc.communicate()
        return Result(None, out.decode("utf-8", "replace"), err.decode("utf-8", "replace"),
                      time.perf_counter() - start)
    return Result(
        proc.returncode,
        out.decode("utf-8", "surrogateescape"),
        err.decode("utf-8", "surrogateescape"),
        time.perf_counter() - start,
    )


# --------------------------------------------------------------------------
# fetch


def download(url: str, dest: Path, sha256: str) -> None:
    if dest.exists() and file_sha256(dest) == sha256:
        print("cached   %s" % rel(dest))
        return
    print("download %s" % url)
    tmp = dest.with_suffix(dest.suffix + ".part")
    with urllib.request.urlopen(url, timeout=60) as resp, open(tmp, "wb") as out:
        shutil.copyfileobj(resp, out)
    got = file_sha256(tmp)
    if got != sha256:
        tmp.unlink()
        die("sha256 mismatch for %s: expected %s, got %s" % (url, sha256, got))
    tmp.replace(dest)


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def safe_extract(archive: Path, dest: Path, top: str) -> Path:
    """Extract an untrusted tarball. Only regular files and directories below
    the single expected top-level directory are accepted; absolute paths,
    '..' components, links and device files abort the extraction."""
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    with tarfile.open(archive, "r:gz") as tar:
        members = tar.getmembers()
        for m in members:
            name = PurePosixPath(m.name)
            if name.is_absolute() or ".." in name.parts or "\\" in m.name:
                die("unsafe path in %s: %r" % (archive.name, m.name))
            if not name.parts or name.parts[0] != top:
                die("unexpected entry outside %s/ in %s: %r" % (top, archive.name, m.name))
            if not (m.isfile() or m.isdir()):
                die("refusing non-regular entry in %s: %r" % (archive.name, m.name))
        for m in members:
            target = dest.joinpath(*PurePosixPath(m.name).parts)
            if not str(target.resolve()).startswith(str(dest.resolve()) + os.sep):
                die("path escapes destination: %r" % m.name)
            if m.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            src = tar.extractfile(m)
            assert src is not None
            with src, open(target, "wb") as out:
                shutil.copyfileobj(src, out)
            # Keep only the user-executable bit; never setuid/setgid.
            target.chmod(0o755 if m.mode & 0o100 else 0o644)
    return dest / top


def make_vars() -> List[str]:
    system = platform.system()
    if system == "Darwin":
        # Equivalent to `make macosx` minus readline: interactive-mode output
        # must not depend on libedit when stdin is a pipe.
        return ["all", "SYSCFLAGS=-DLUA_USE_MACOSX", "SYSLIBS="]
    if system == "Linux":
        return ["linux"]  # 5.4's `linux` target is linux-noreadline
    return ["posix"]


def cmd_fetch(args: argparse.Namespace) -> int:
    cache: Path = args.cache.resolve()
    downloads = cache / "downloads"
    downloads.mkdir(parents=True, exist_ok=True)
    src_tgz = downloads / ("lua-%s.tar.gz" % LUA_VERSION)
    tests_tgz = downloads / ("lua-%s-tests.tar.gz" % LUA_VERSION)
    download(LUA_URL, src_tgz, LUA_SHA256)
    download(TESTS_URL, tests_tgz, TESTS_SHA256)

    src = safe_extract(src_tgz, cache / "src", "lua-%s" % LUA_VERSION)
    tests = safe_extract(tests_tgz, cache / "tests", "lua-%s-tests" % LUA_VERSION)
    print("extracted %s and %s" % (rel(src), rel(tests)))

    jobs = str(os.cpu_count() or 2)
    cmd = ["make", "-j", jobs, "-C", str(src / "src")] + make_vars()
    print("build    " + " ".join(cmd))
    subprocess.run(cmd, check=True, stdout=None if args.verbose else subprocess.DEVNULL)
    bindir = cache / "bin"
    bindir.mkdir(exist_ok=True)
    for exe in ("lua", "luac"):
        shutil.copy2(src / "src" / exe, bindir / exe)
    print("built    %s, %s" % (rel(bindir / "lua"), rel(bindir / "luac")))

    if not args.no_libs:
        build_test_libs(src / "src", tests / "libs", args.verbose)
    return 0


def build_test_libs(lua_inc: Path, libs: Path, verbose: bool) -> None:
    """Build the C modules attrib.lua loads when _port is false."""
    cflags = "-Wall -std=gnu99 -O2 -I%s -fPIC -shared" % lua_inc
    if platform.system() == "Darwin":
        cflags += " -undefined dynamic_lookup"
    cc = os.environ.get("CC", "cc")
    cmd = ["make", "-C", str(libs), "LUA_DIR=%s" % lua_inc, "CC=%s" % cc, "CFLAGS=%s" % cflags]
    print("build    test C libraries in %s" % rel(libs))
    proc = subprocess.run(cmd, stdout=None if verbose else subprocess.DEVNULL,
                          stderr=None if verbose else subprocess.PIPE)
    if proc.returncode != 0:
        print("warning: building test C libraries failed (only needed with --no-port):")
        if proc.stderr:
            print(proc.stderr.decode("utf-8", "replace")[-2000:])


# --------------------------------------------------------------------------
# suite


class SuiteFile(NamedTuple):
    name: str
    mode: str  # "dump", "dump-strip", "source", "big"
    returns: Optional[str]  # Lua expression the file's first result must equal
    soft_skip: bool = False  # all.lua skips it when _soft is set


# Files in the order all.lua runs them, loaded the same way all.lua loads
# them: its local `dofile` compiles, dumps (optionally stripped) and reloads
# the chunk; strings.lua/literals.lua use the original dofile; gc.lua uses a
# plain loadfile; big.lua runs inside a coroutine.
SUITE_FILES = [
    SuiteFile("main.lua", "dump", None),
    SuiteFile("gc.lua", "source", None),
    SuiteFile("db.lua", "dump", None),
    SuiteFile("calls.lua", "dump", "deep"),
    SuiteFile("strings.lua", "source", None),
    SuiteFile("literals.lua", "source", None),
    SuiteFile("tpack.lua", "dump", None),
    SuiteFile("attrib.lua", "dump", "27"),
    SuiteFile("gengc.lua", "dump", None),
    SuiteFile("locals.lua", "dump", "5"),
    SuiteFile("constructs.lua", "dump", None),
    SuiteFile("code.lua", "dump-strip", None),
    SuiteFile("big.lua", "big", None, soft_skip=True),
    SuiteFile("cstack.lua", "dump", None),
    SuiteFile("nextvar.lua", "dump", None),
    SuiteFile("pm.lua", "dump", None),
    SuiteFile("utf8.lua", "dump", None),
    SuiteFile("api.lua", "dump", None),
    SuiteFile("events.lua", "dump", "12"),
    SuiteFile("vararg.lua", "dump", None),
    SuiteFile("closure.lua", "dump", None),
    SuiteFile("coroutine.lua", "dump", None),
    SuiteFile("goto.lua", "dump-strip", None),
    SuiteFile("errors.lua", "dump", None),
    SuiteFile("math.lua", "dump", None),
    SuiteFile("sort.lua", "dump-strip", None),
    SuiteFile("bitwise.lua", "dump", None),
    SuiteFile("verybig.lua", "dump-strip", "10"),
    SuiteFile("files.lua", "dump", None),
]
EXTRA_SUITE_FILES = {"heavy.lua": SuiteFile("heavy.lua", "source", None)}

# A file whose output contains one of these markers returned early without
# testing anything; report it as SKIP rather than PASS.
WHOLE_FILE_SKIPS = {
    "api.lua": "testC not active: skipping API tests",
    "code.lua": "testC not active: skipping opcode tests",
}

OK_MARKER = "@@lua-suite-harness: file finished OK@@"
SKIP_MARKER = "@@lua-suite-harness: not performed: "


class SuitePatch(NamedTuple):
    file: str
    old: str
    new: str
    why: str
    only_if: str = ""  # name of the suite option that enables the patch ("" = always)


# Minimal edits applied to a scratch copy of the suite before each run. Every
# `old` text must occur exactly once, so a changed upstream file fails loudly.
SUITE_PATCHES = [
    SuitePatch(
        "main.lua",
        "assert(string.find(t, [[\n1 --\n2a = 2\n3\n]], 1, true))",
        "assert(string.find(t, [[\n1 --\n2a = 2\n3\n]], 1, true) or\n"
        "       string.find(t, \"123\\n\", 1, true))  -- harness: no readline echo",
        "GNU readline echoes piped input after the prompt; lua.c without readline"
        " (and a non-readline implementation) prints only the prompts",
    ),
    SuitePatch(
        "main.lua",
        "RUN(\"env LUA_CPATH='./libs/?.so' lua -l lib2-v2 -e 'print(lib2.id())' > %s\",\n"
        "    out)\ncheckout(\"true\\n\")",
        "-- harness --no-clibs: skipped loading C module libs/lib2-v2.so",
        "the -l test with a versioned C module needs C library loading",
        only_if="no_clibs",
    ),
]

DRIVER_TEMPLATE = r"""-- generated by scripts/test_lua.py; mirrors the per-file setup of all.lua
local file, mode, expected = %(file)s, %(mode)s, %(expected)s
_G.ARG = arg
_soft = %(soft)s
_port = %(port)s
_nomsg = %(nomsg)s
debug = nil
T = rawget(_G, "T")
function Message (m)
  -- the harness marker is printed even with _nomsg so skips stay visible
  print(%(skip_marker)s .. m:gsub("\n", " "))
  if not _nomsg then print(m) end
end
assert(os.setlocale"C")
print("\n***** FILE '" .. file .. "'*****")
local res
if mode == "big" then
  local f = coroutine.wrap(assert(loadfile(file)))
  assert(f() == 'b')
  assert(f() == 'a')
elseif mode == "source" then
  res = assert(loadfile(file))()
else
  local f = assert(loadfile(file))
  if %(dump)s then
    f = assert(load(string.dump(f, mode == "dump-strip")))
  end
  res = f()
end
if expected ~= nil then
  local want = assert(load("return " .. expected))()
  assert(res == want, string.format("%%s returned %%s, expected %%s (%%s)",
         file, tostring(res), tostring(want), expected))
end
print("\n" .. %(marker)s)
"""


def lua_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def lua_bool(b: bool) -> str:
    return "true" if b else "false"


def apply_suite_patches(work: Path, args: argparse.Namespace) -> List[SuitePatch]:
    applied = []
    for patch in SUITE_PATCHES:
        if patch.only_if and not getattr(args, patch.only_if):
            continue
        path = work / patch.file
        text = path.read_text(encoding="latin-1")
        if text.count(patch.old) != 1:
            die("suite patch for %s no longer applies (%s)" % (patch.file, patch.why))
        path.write_text(text.replace(patch.old, patch.new), encoding="latin-1")
        applied.append(patch)
    return applied


def cmd_suite(args: argparse.Namespace) -> int:
    cache: Path = args.cache.resolve()
    tests_dir = cache / "tests" / ("lua-%s-tests" % LUA_VERSION)
    if not tests_dir.is_dir():
        die("official test suite not found in %s; run `fetch` first" % rel(tests_dir))
    lua = resolve_interp(args.lua, cache, DEFAULT_LUA)
    soft = not args.no_soft
    port = not args.no_port

    by_name = {f.name: f for f in SUITE_FILES}
    by_name.update(EXTRA_SUITE_FILES)
    if args.files:
        selected = []
        for name in args.files.split(","):
            name = name.strip()
            if not name.endswith(".lua"):
                name += ".lua"
            if name not in by_name:
                die("unknown suite file %s (known: %s)" % (name, ", ".join(sorted(by_name))))
            selected.append(by_name[name])
    else:
        selected = list(SUITE_FILES)

    rows: List[Tuple[str, str, float, str]] = []
    failures: List[Tuple[str, Result]] = []
    with tempfile.TemporaryDirectory(prefix="lua-suite-") as tmp:
        # Run from a scratch copy: the suite writes files next to itself and
        # some files get harness patches (see SUITE_PATCHES).
        work = Path(tmp) / "tests"
        shutil.copytree(str(tests_dir), str(work))
        applied = apply_suite_patches(work, args)
        drivers = Path(tmp) / "drivers"
        drivers.mkdir()
        for sf in selected:
            if sf.soft_skip and soft:
                rows.append((sf.name, "SKIP", 0.0, "skipped by all.lua when _soft"))
                continue
            # main.lua tests the stand-alone interpreter; it only runs when
            # _port is false, which is fine on any POSIX host.
            file_port = port
            if sf.name == "main.lua" and not args.port_main and os.name == "posix":
                file_port = False
            driver = drivers / ("run_" + sf.name)
            driver.write_text(DRIVER_TEMPLATE % {
                "file": lua_str(sf.name),
                "mode": lua_str(sf.mode),
                "expected": lua_str(sf.returns) if sf.returns else "nil",
                "soft": lua_bool(soft),
                "port": lua_bool(file_port),
                "nomsg": lua_bool(args.nomsg),
                "dump": lua_bool(not args.no_dump),
                "marker": lua_str(OK_MARKER),
                "skip_marker": lua_str(SKIP_MARKER),
            })
            if args.verbose:
                print("run %s (cwd %s)" % (" ".join([str(lua), str(driver)]), work))
            res = run([str(lua), str(driver)], work, args.timeout)
            notes = [m.strip(" \t>\n<") for m in re.findall(re.escape(SKIP_MARKER) + r"([^\n]*)", res.stdout)]
            if res.code is None:
                status = "TIMEOUT"
            elif res.code == 0 and OK_MARKER in res.stdout:
                status = "PASS"
                marker = WHOLE_FILE_SKIPS.get(sf.name)
                if marker and marker in res.stdout:
                    status = "SKIP"
            else:
                status = "FAIL"
            if status in ("FAIL", "TIMEOUT"):
                failures.append((sf.name, res))
            if not file_port and port:
                notes.insert(0, "_port=false")
            rows.append((sf.name, status, res.seconds, "; ".join(notes)))
            print("%-16s %-8s %7.2fs  %s" % (sf.name, status, res.seconds, rows[-1][3]), flush=True)

    print()
    print("interpreter: %s" % lua)
    print("globals: _soft=%s _port=%s _nomsg=%s, chunks %s"
          % (lua_bool(soft), lua_bool(port), lua_bool(args.nomsg),
             "loaded from source" if args.no_dump else "round-tripped through string.dump as in all.lua"))
    for patch in applied:
        print("patched %s: %s" % (patch.file, patch.why))
    counts: Dict[str, int] = {}
    for _, status, _, _ in rows:
        counts[status] = counts.get(status, 0) + 1
    print("summary: " + ", ".join("%d %s" % (n, s) for s, n in sorted(counts.items())))
    for name, res in failures:
        print("\n---- %s: %s ----" % (name, "timeout after %gs" % args.timeout if res.code is None
                                       else "exit status %s" % res.code))
        text = (res.stderr.strip() or res.stdout.strip()).splitlines()
        for line in text[: args.lines]:
            print("  " + line)
        if len(text) > args.lines:
            print("  ... (%d more lines)" % (len(text) - args.lines))
    return 1 if failures else 0


# --------------------------------------------------------------------------
# corpus: directives, normalization, expected files


class Directives(NamedTuple):
    opts: List[str]   # interpreter options before the script name
    args: List[str]   # script arguments
    cmd: Optional[List[str]]  # full argument list replacing opts/script/args
    stdin: Optional[str]  # file (relative to the script, or {script}) fed on stdin
    stderr: str       # "message" (default), "full" or "ignore"
    env: Dict[str, str]  # extra environment variables


def parse_directives(path: Path) -> Directives:
    """Directives are `--! key: value` lines in the leading comment block of
    a test script (before the first line of code):

      --! opts: -W -e "x=1"     interpreter options placed before the script
      --! args: a "b c"         script arguments placed after it
      --! cmd: -e "print(1)" -  complete argument list (no script appended);
                                {script} expands to the script's file name
      --! stdin: fixtures/f.txt file fed to stdin ({script} = the test itself)
      --! stderr: message       compare stderr up to 'stack traceback:'
                                (default), 'full', or 'ignore'
      --! env: NAME=value ...   extra environment variables (for example
                                DODO_LUA_GC_STRESS=1, which the reference
                                interpreter ignores)
    """
    opts: List[str] = []
    args: List[str] = []
    cmd: Optional[List[str]] = None
    stdin = None
    stderr = "message"
    env: Dict[str, str] = {}
    with open(path, encoding="utf-8", errors="replace") as f:
        for lineno, line in enumerate(f, 1):
            if (lineno == 1 and line.startswith("#")) or not line.strip():
                continue
            m = re.match(r"--!\s*(\w+)\s*:\s*(.*?)\s*$", line)
            if not m:
                if line.startswith("--") and not line.startswith("--[["):
                    continue  # directives may follow the header comment
                break
            key, value = m.group(1), m.group(2)
            if key == "opts":
                opts = shlex.split(value)
            elif key == "args":
                args = shlex.split(value)
            elif key == "cmd":
                cmd = shlex.split(value)
            elif key == "stdin":
                stdin = value
            elif key == "stderr":
                if value not in ("message", "full", "ignore"):
                    die("%s: bad stderr directive %r" % (path, value))
                stderr = value
            elif key == "env":
                for item in shlex.split(value):
                    name, eq, val = item.partition("=")
                    if not eq or not name:
                        die("%s: bad env directive %r" % (path, item))
                    env[name] = val
            else:
                die("%s: unknown directive %r" % (path, key))
    return Directives(opts, args, cmd, stdin, stderr, env)


def run_script(lua: Path, script: Path, timeout: float) -> Result:
    d = parse_directives(script)
    stdin = None
    if d.stdin:
        stdin = (script.parent / d.stdin.replace("{script}", script.name)).read_bytes()
    if d.cmd is not None:
        argv = [str(lua)] + [a.replace("{script}", script.name) for a in d.cmd]
    else:
        argv = [str(lua)] + d.opts + [script.name] + d.args
    return run(argv, script.parent, timeout, stdin, d.env)


def normalize_stderr(text: str, lua: Path, mode: str) -> str:
    """Make stderr comparable across interpreters: the program name that
    lua.c prefixes to messages becomes `lua:`, other occurrences of the full
    interpreter path become `lua`, and unless mode is 'full' everything from
    the first 'stack traceback:' line on is dropped."""
    if mode == "ignore":
        return ""
    names = sorted({str(lua), lua.name}, key=len, reverse=True)
    out_lines = []
    for line in text.splitlines(True):
        for name in names:
            if line.startswith(name + ": "):
                line = "lua: " + line[len(name) + 2:]
                break
        # e.g. "usage: /path/to/lua [options] [script [args]]"
        line = line.replace(str(lua), "lua")
        if mode == "message" and line.rstrip("\r\n") == "stack traceback:":
            break
        out_lines.append(line)
    return "".join(out_lines)


class Expected(NamedTuple):
    code: int
    stdout: str
    stderr: str


SECTION_RE = re.compile(r"^@@(exit|stdout|stderr)(?: (.*))?$")


def format_expected(code: int, stdout: str, stderr: str) -> str:
    parts = ["@@exit %d\n" % code]
    for name, text in (("stdout", stdout), ("stderr", stderr)):
        if name == "stderr" and not text:
            continue
        noeol = bool(text) and not text.endswith("\n")
        parts.append("@@%s%s\n" % (name, " noeol" if noeol else ""))
        parts.append(text + ("\n" if noeol else ""))
    return "".join(parts)


def parse_expected(path: Path) -> Expected:
    code = 0
    sections: Dict[str, List[str]] = {"stdout": [], "stderr": []}
    noeol: Dict[str, bool] = {}
    current: Optional[str] = None
    with open(path, encoding="utf-8", errors="surrogateescape", newline="") as f:
        data = f.read()
    # Split on "\n" only: output may legitimately contain lone "\r".
    for line in data.split("\n")[:-1] if data.endswith("\n") else data.split("\n"):
        m = SECTION_RE.match(line)
        if m:
            if m.group(1) == "exit":
                code = int(m.group(2))
                current = None
            else:
                current = m.group(1)
                noeol[current] = m.group(2) == "noeol"
            continue
        if current is None:
            die("%s: content outside a section: %r" % (path, line))
        sections[current].append(line + "\n")
    texts = {}
    for name, lines in sections.items():
        t = "".join(lines)
        if noeol.get(name) and t.endswith("\n"):
            t = t[:-1]
        texts[name] = t
    return Expected(code, texts["stdout"], texts["stderr"])


def collect_scripts(paths: Sequence[str], default_dir: Path) -> List[Path]:
    if not paths:
        return sorted(p for p in default_dir.glob("*.lua"))
    out: List[Path] = []
    for p in paths:
        path = Path(p).resolve()
        if path.is_dir():
            out.extend(sorted(path.glob("*.lua")))
        elif path.is_file():
            out.append(path)
        else:
            candidate = default_dir / (p if p.endswith(".lua") else p + ".lua")
            if candidate.is_file():
                out.append(candidate)
            else:
                matches = sorted(default_dir.glob(p if p.endswith(".lua") else p + "*.lua"))
                if not matches:
                    die("no such test: %s" % p)
                out.extend(matches)
    return out


def describe(code: Optional[int]) -> str:
    if code is None:
        return "timeout"
    if code < 0:
        return "signal %d" % -code
    return "exit %d" % code


def compare(name: str, want: Expected, got: Result, stderr_mode: str,
            want_label: str, got_label: str) -> List[str]:
    """Return a list of human-readable mismatch reports (empty if equal)."""
    problems = []
    if got.code != want.code:
        problems.append("%s: %s, %s: %s" % (want_label, describe(want.code), got_label, describe(got.code)))
    for stream, a, b in (("stdout", want.stdout, got.stdout), ("stderr", want.stderr, got.stderr)):
        if stream == "stderr" and stderr_mode == "ignore":
            continue
        if a != b:
            diff = difflib.unified_diff(
                a.splitlines(True), b.splitlines(True),
                fromfile="%s %s (%s)" % (name, stream, want_label),
                tofile="%s %s (%s)" % (name, stream, got_label), n=2)
            lines = [l if l.endswith("\n") else l + "\n\\ No newline at end of file\n" for l in diff]
            problems.append("".join(lines).rstrip("\n"))
    return problems


def print_report(results: List[Tuple[str, List[str]]], max_lines: int) -> int:
    failed = [(n, p) for n, p in results if p]
    for name, problems in failed:
        print("\n==== %s ====" % name)
        text = "\n".join(problems).splitlines()
        for line in text[:max_lines]:
            print(line)
        if len(text) > max_lines:
            print("... (%d more lines)" % (len(text) - max_lines))
    print("\n%d/%d passed" % (len(results) - len(failed), len(results)))
    if failed:
        print("failed: " + " ".join(n for n, _ in failed))
    return 1 if failed else 0


def expected_from_run(res: Result, lua: Path, mode: str) -> Expected:
    assert res.code is not None
    return Expected(res.code, res.stdout, normalize_stderr(res.stderr, lua, mode))


def cmd_diff(args: argparse.Namespace) -> int:
    cache = args.cache.resolve()
    lua = resolve_interp(args.lua, cache, DEFAULT_LUA)
    ref = resolve_interp(args.ref, cache, cache / "bin" / "lua")
    results = []
    for script in collect_scripts(args.paths, CORPUS_DIR):
        mode = "full" if args.strict_stderr else parse_directives(script).stderr
        r = run_script(ref, script, args.timeout)
        g = run_script(lua, script, args.timeout)
        if r.code is None:
            results.append((script.name, ["reference timed out"]))
            continue
        want = expected_from_run(r, ref, mode)
        got = Result(g.code, g.stdout, normalize_stderr(g.stderr, lua, mode), g.seconds)
        problems = compare(script.name, want, got, mode, "ref", "lua")
        results.append((script.name, problems))
        print("%-40s %s" % (script.name, "ok" if not problems else "MISMATCH"), flush=True)
    return print_report(results, args.lines)


def cmd_expect(args: argparse.Namespace) -> int:
    cache = args.cache.resolve()
    ref = resolve_interp(args.ref, cache, cache / "bin" / "lua")
    bad = 0
    for script in collect_scripts(args.paths, CORPUS_DIR):
        mode = parse_directives(script).stderr
        r1 = run_script(ref, script, args.timeout)
        r2 = run_script(ref, script, args.timeout)
        problem = None
        if r1.code is None:
            problem = "timed out"
        elif (r1.code, r1.stdout, normalize_stderr(r1.stderr, ref, mode)) != \
                (r2.code, r2.stdout, normalize_stderr(r2.stderr, ref, mode)):
            problem = "output is not deterministic across two runs"
        elif any(SECTION_RE.match(l) for l in (r1.stdout + "\n" + r1.stderr).split("\n")):
            problem = "output contains a line that looks like an @@ section header"
        elif r1.code != 0 and mode == "ignore":
            problem = "nonzero exit with stderr ignored; use `--! stderr: message`"
        if problem:
            print("%-40s ERROR: %s" % (script.name, problem))
            bad += 1
            continue
        exp = expected_from_run(r1, ref, mode)
        content = format_expected(exp.code, exp.stdout, exp.stderr)
        target = script.with_suffix(".expected")
        old = target.read_text(encoding="utf-8", errors="surrogateescape") if target.exists() else None
        if old != content:
            with open(target, "w", encoding="utf-8", errors="surrogateescape", newline="") as f:
                f.write(content)
        status = "unchanged" if old == content else ("updated" if old is not None else "created")
        extra = "" if exp.code == 0 else "  (%s)" % describe(exp.code)
        print("%-40s %s%s" % (script.name, status, extra))
    if not args.paths:
        for stale in sorted(CORPUS_DIR.glob("*.expected")):
            if not stale.with_suffix(".lua").exists():
                print("%-40s stale (no matching .lua); remove it" % stale.name)
                bad += 1
    return 1 if bad else 0


def cmd_check(args: argparse.Namespace) -> int:
    cache = args.cache.resolve()
    lua = resolve_interp(args.lua, cache, DEFAULT_LUA)
    results = []
    for script in collect_scripts(args.paths, CORPUS_DIR):
        exp_path = script.with_suffix(".expected")
        if not exp_path.exists():
            results.append((script.name, ["missing %s (run `expect`)" % exp_path.name]))
            print("%-40s MISSING EXPECTED" % script.name)
            continue
        mode = parse_directives(script).stderr
        want = parse_expected(exp_path)
        g = run_script(lua, script, args.timeout)
        got = Result(g.code, g.stdout, normalize_stderr(g.stderr, lua, mode), g.seconds)
        problems = compare(script.name, want, got, mode, "expected", "lua")
        results.append((script.name, problems))
        print("%-40s %s" % (script.name, "ok" if not problems else "FAIL"), flush=True)
    return print_report(results, args.lines)


# --------------------------------------------------------------------------
# bench


def cmd_bench(args: argparse.Namespace) -> int:
    cache = args.cache.resolve()
    lua = resolve_interp(args.lua, cache, DEFAULT_LUA)
    ref = resolve_interp(args.ref, cache, cache / "bin" / "lua")
    scripts = collect_scripts(args.paths, BENCH_DIR)
    rows = []
    mismatches = 0
    for script in scripts:
        times = {}
        outputs = {}
        for label, interp in (("ref", ref), ("lua", lua)):
            best = None
            for _ in range(args.repeat):
                res = run([str(interp), script.name], script.parent, args.timeout)
                if res.code != 0:
                    best = None
                    outputs[label] = "<%s> %s" % (describe(res.code), res.stderr.strip()[:200])
                    break
                outputs[label] = res.stdout
                best = res.seconds if best is None else min(best, res.seconds)
            times[label] = best
        same = outputs.get("ref") == outputs.get("lua")
        if not same or times["ref"] is None or times["lua"] is None:
            mismatches += 1
        rows.append((script.stem, times["ref"], times["lua"], same, outputs))
        print("%-24s ref %s  lua %s%s" % (
            script.stem, fmt_time(times["ref"]), fmt_time(times["lua"]),
            "" if same else "  OUTPUT MISMATCH"), file=sys.stderr, flush=True)

    print()
    print("| benchmark | reference (s) | tested (s) | ratio |")
    print("|---|---:|---:|---:|")
    ratios = []
    for name, t_ref, t_lua, same, _ in rows:
        ratio = ""
        if t_ref and t_lua:
            ratios.append(t_lua / t_ref)
            ratio = "%.2fx" % (t_lua / t_ref)
        print("| %s%s | %s | %s | %s |" % (name, "" if same else " (output differs)",
                                         fmt_time(t_ref), fmt_time(t_lua), ratio))
    if ratios:
        geo = 1.0
        for r in ratios:
            geo *= r
        geo **= 1.0 / len(ratios)
        print("| **geometric mean** | | | **%.2fx** |" % geo)
    print("\nreference: %s\ntested:    %s" % (ref, lua))
    print("best of %d runs; ratio = tested time / reference time (lower is better)" % args.repeat)
    for name, _, _, same, outputs in rows:
        if not same:
            print("\n%s output differs:\n  ref: %r\n  lua: %r" % (
                name, outputs.get("ref", "")[:300], outputs.get("lua", "")[:300]))
    return 1 if mismatches else 0


def fmt_time(t: Optional[float]) -> str:
    return "failed" if t is None else "%.3f" % t


# --------------------------------------------------------------------------


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--cache", type=Path, default=DEFAULT_CACHE,
                        help="reference build cache (default: target/lua-reference)")
    sub = parser.add_subparsers(dest="command")
    sub.required = True

    lua_help = "interpreter under test (default: build/lua; 'ref' = reference build)"
    ref_help = "reference interpreter (default: <cache>/bin/lua)"

    p = sub.add_parser("fetch", help="download and build reference Lua %s" % LUA_VERSION)
    p.add_argument("--no-libs", action="store_true", help="skip the test suite's C libraries")
    p.add_argument("-v", "--verbose", action="store_true")
    p.set_defaults(func=cmd_fetch)

    p = sub.add_parser("suite", help="run the official Lua test suite file by file")
    p.add_argument("--lua", help=lua_help)
    p.add_argument("--files", help="comma-separated suite files (default: everything all.lua runs)")
    p.add_argument("--timeout", type=float, default=300.0, help="seconds per file (default 300)")
    p.add_argument("--no-soft", action="store_true", help="_soft=false: include long/memory-hungry tests")
    p.add_argument("--no-port", action="store_true",
                   help="_port=false everywhere: non-portable tests (needs the C libs from fetch)")
    p.add_argument("--port-main", action="store_true",
                   help="keep _port=true for main.lua too (skips stand-alone interpreter tests)")
    p.add_argument("--nomsg", action="store_true", help="_nomsg=true: hide 'test not performed' notes")
    p.add_argument("--no-clibs", action="store_true",
                   help="skip main.lua's C-module (-l lib2-v2) check for implementations without loadlib")
    p.add_argument("--no-dump", action="store_true",
                   help="load files from source instead of round-tripping through string.dump")
    p.add_argument("--lines", type=int, default=15, help="failure output lines to show (default 15)")
    p.add_argument("-v", "--verbose", action="store_true")
    p.set_defaults(func=cmd_suite)

    for name, func, help_text in (
        ("diff", cmd_diff, "compare our interpreter against the reference on tests/lua"),
        ("expect", cmd_expect, "regenerate tests/lua/*.expected from the reference"),
        ("check", cmd_check, "compare our interpreter against tests/lua/*.expected"),
    ):
        p = sub.add_parser(name, help=help_text)
        if name in ("diff", "check"):
            p.add_argument("--lua", help=lua_help)
        if name in ("diff", "expect"):
            p.add_argument("--ref", help=ref_help)
        if name == "diff":
            p.add_argument("--strict-stderr", action="store_true",
                           help="compare full stderr including tracebacks")
        p.add_argument("--timeout", type=float, default=60.0, help="seconds per script (default 60)")
        p.add_argument("--lines", type=int, default=60, help="diff lines to show per failure")
        p.add_argument("paths", nargs="*", help="scripts, directories or name prefixes (default: tests/lua)")
        p.set_defaults(func=func)

    p = sub.add_parser("bench", help="time benchmarks/lua under both interpreters")
    p.add_argument("--lua", help=lua_help)
    p.add_argument("--ref", help=ref_help)
    p.add_argument("--repeat", type=int, default=3, help="runs per benchmark; best is reported")
    p.add_argument("--timeout", type=float, default=600.0)
    p.add_argument("paths", nargs="*", help="benchmarks or name prefixes (default: benchmarks/lua)")
    p.set_defaults(func=cmd_bench)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
