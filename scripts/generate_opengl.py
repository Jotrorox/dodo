#!/usr/bin/env python3
"""Generate stdlib/std/gpu/opengl/gl.dodo from the Khronos OpenGL registry.

The output is the raw OpenGL binding: every enum and command of the OpenGL
4.6 core profile, one to one. Commands are reached through a table of
function addresses that `gl.load` fills from a context's loader, so a
program links no OpenGL library and starts on machines without one.

Names follow Dodo style: `glTexImage2D` becomes `gl.tex_image_2d`,
`GL_COLOR_BUFFER_BIT` becomes `gl.COLOR_BUFFER_BIT`, and parameters are
snake_case. Reserved words get a trailing underscore (`type_`).

The registry is pinned: without --registry, gl.xml is downloaded from the
OpenGL-Registry commit below and its SHA-256 verified. To move to a newer
registry, update COMMIT and SHA256 together and run --write.
Run with --check to verify the checked-in file or --write to regenerate it.
"""

import argparse
import hashlib
import sys
import tempfile
import urllib.request
import xml.etree.ElementTree as ElementTree
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "stdlib/std/gpu/opengl/gl.dodo"
API = "gl"
PROFILE = "core"
VERSION = (4, 6)
COMMIT = "6af574a14089ccfee87efe230ebcdd8742859813"
SHA256 = "b9ca2cfa5c676e901c20d34af3407f1687cde0f1336a5ff7a8974d04c7494ad3"
URL = f"https://raw.githubusercontent.com/KhronosGroup/OpenGL-Registry/{COMMIT}/xml/gl.xml"

# Dodo's reserved words, plus contextual names a parameter must not shadow.
RESERVED = {
    "package", "import", "pub", "fn", "struct", "enum", "interface", "type",
    "dyn", "const", "let", "return", "if", "else", "for", "in", "break",
    "continue", "match", "unsafe", "extern", "as", "from", "stores",
    "requires_plain", "static", "void", "mut", "true", "false", "asm",
    "self", "Self", "some", "none", "ok", "err", "core",
}

SCALARS = {
    "GLenum": "u32",
    "GLboolean": "u8",
    "GLbitfield": "u32",
    "GLbyte": "i8",
    "GLubyte": "u8",
    "GLshort": "i16",
    "GLushort": "u16",
    "GLint": "i32",
    "GLuint": "u32",
    "GLsizei": "i32",
    "GLfloat": "f32",
    "GLclampf": "f32",
    "GLdouble": "f64",
    "GLclampd": "f64",
    "GLchar": "u8",
    "GLhalf": "u16",
    "GLfixed": "i32",
    "GLintptr": "isize",
    "GLsizeiptr": "isize",
    "GLint64": "i64",
    "GLuint64": "u64",
    # An opaque driver handle; `void` as a pointee is a byte.
    "GLsync": "*mut u8",
    "void": "u8",
}

DEBUG_PROC = 'extern "C" fn(u32, u32, u32, u32, i32, *const u8, *const u8)'


def snake(name: str) -> str:
    """`TexImage2D` -> `tex_image_2d`, `VertexAttribI4ui` -> `vertex_attrib_i4ui`."""
    out = []
    for index, char in enumerate(name):
        previous = name[index - 1] if index else ""
        following = name[index + 1] if index + 1 < len(name) else ""
        if char.isupper() and (
            previous.islower() or (previous.isupper() and following.islower())
        ):
            # A new word: after a lowercase letter, or the last capital of an
            # acronym that starts one (`2DMultisample`, `IPointer`).
            out.append("_")
        elif char.isdigit() and previous.isalpha():
            # A lone capital before a digit is a type prefix (I4ui, P4uiv, L1d).
            lone = previous.isupper() and (index < 2 or not name[index - 2].isupper())
            # Matrix dimensions stay together (`Matrix2x3fv`).
            dimension = previous == "x" and index >= 2 and name[index - 2].isdigit()
            if not lone and not dimension:
                out.append("_")
        out.append(char.lower())
    return "".join(out).replace("__", "_")


def identifier(name: str) -> str:
    name = snake(name)
    return name + "_" if name in RESERVED else name


def c_type(element: ElementTree.Element) -> str:
    """The C type text of a <proto> or <param>, without its name."""
    text = element.text or ""
    for child in element:
        if child.tag == "name":
            break
        text += (child.text or "") + (child.tail or "")
    return " ".join(text.split())


def dodo_type(text: str, context: str) -> str:
    text = text.replace("const", " const ").replace("*", " * ")
    tokens = text.split()
    if tokens == ["GLDEBUGPROC"]:
        return DEBUG_PROC
    base = [token for token in tokens if token not in ("const", "*", "struct")]
    if len(base) != 1 or base[0] not in SCALARS:
        raise SystemExit(f"{context}: unsupported C type {text!r}")
    result = SCALARS[base[0]]
    if "*" not in tokens:
        if result == "u8" and base[0] == "void":
            return "void"
        return result
    # Build from the inside out. The first pointer's pointee is the base,
    # constant when `const` comes before the first `*`; each later pointer's
    # pointee is the previous pointer, constant when `const` follows its `*`.
    stars = [index for index, token in enumerate(tokens) if token == "*"]
    for level, star in enumerate(stars):
        before = stars[level - 1] + 1 if level else 0
        constant = "const" in tokens[before:star]
        result = ("*const " if constant else "*mut ") + result
    return result


def collect(registry: ElementTree.Element):
    commands = {}
    for command in registry.find("commands"):
        proto = command.find("proto")
        name = proto.find("name").text
        commands[name] = command
    enums = {}
    for group in registry.findall("enums"):
        bitmask = group.get("type") == "bitmask"
        for value in group.findall("enum"):
            api = value.get("api")
            if api and api != API:
                continue
            enums[value.get("name")] = (value.get("value"), value.get("type"), bitmask)

    wanted_commands = []
    wanted_enums = []
    seen_commands = set()
    seen_enums = set()
    removed_commands = set()
    removed_enums = set()
    for feature in registry.findall("feature"):
        if feature.get("api") != API:
            continue
        number = tuple(int(part) for part in feature.get("number").split("."))
        if number > VERSION:
            continue
        for require in feature.findall("require"):
            if require.get("profile") not in (None, PROFILE):
                continue
            for item in require:
                name = item.get("name")
                if item.tag == "command" and name not in seen_commands:
                    seen_commands.add(name)
                    wanted_commands.append((name, number))
                elif item.tag == "enum" and name not in seen_enums:
                    seen_enums.add(name)
                    wanted_enums.append(name)
        for remove in feature.findall("remove"):
            if remove.get("profile") not in (None, PROFILE):
                continue
            for item in remove:
                if item.tag == "command":
                    removed_commands.add(item.get("name"))
                elif item.tag == "enum":
                    removed_enums.add(item.get("name"))
    commands_out = [
        (name, number, commands[name]) for name, number in wanted_commands if name not in removed_commands
    ]
    enums_out = [(name, enums[name]) for name in wanted_enums if name not in removed_enums]
    return commands_out, enums_out


def render(registry_path: Path) -> str:
    registry = ElementTree.parse(registry_path).getroot()
    commands, enums = collect(registry)
    lines = [
        "package gl",
        "",
        "// GENERATED by scripts/generate_opengl.py from the Khronos OpenGL registry",
        f"// (gl.xml at OpenGL-Registry {COMMIT[:12]}, Apache-2.0). Do not edit;",
        "// regenerate instead.",
        "//",
        f"// The OpenGL {VERSION[0]}.{VERSION[1]} core profile, one to one: every enum is a",
        "// constant and every command an unsafe function. `glClearColor` is",
        "// `gl.clear_color` and `GL_COLOR_BUFFER_BIT` is `gl.COLOR_BUFFER_BIT`.",
        "// GLboolean is u8, GLsizei i32, GLintptr and GLsizeiptr isize, GLsync a raw",
        "// pointer and `void *` a byte pointer. Constants are u32 (u64 when the",
        "// registry says so); cast them where a command takes GLint.",
        "//",
        "// Commands go through a table of addresses that load() fills with the",
        "// current context's entry points; std/gpu/opengl does that when it makes a",
        "// context current. Calling a command that is not loaded, because no context",
        "// was made current or the driver lacks it (macOS stops at 4.1), panics.",
        "// Like the C API, the commands are unsafe: they take raw pointers, act on",
        "// the calling thread's current context and leave validity to the caller.",
        "",
        "import \"core/bytes\"",
        "import \"core/mem\"",
        "import \"core/ptr\"",
        "",
        f"pub const COMMAND_COUNT: usize = {len(commands)}",
        "",
        f"static mut ADDRESSES: [{len(commands)}]usize = [0; {len(commands)}]",
        "",
        "// The registry name of every command, NUL-terminated, in table order.",
    ]
    names = "".join(name + "\\0" for name, _, _ in commands)
    lines.append(f'const NAMES: &[u8] = b"{names}"')
    lines += [
        "",
        "// Fill the table: resolve receives each command's NUL-terminated registry",
        "// name (\"glClearColor\") and returns its address or null. Returns how many",
        "// commands resolved. Entries resolve() does not know are cleared, so a",
        "// table loaded for one context never keeps another's addresses.",
        "pub unsafe fn load(resolve: &mut dyn fn(*const u8) -> usize) -> usize {",
        "    loaded := 0usize",
        "    start := 0usize",
        "    index := 0usize",
        "    for index < COMMAND_COUNT {",
        "        end := start",
        "        for NAMES[end] != 0 {",
        "            end += 1",
        "        }",
        "        address := resolve(ptr.as_ptr(&NAMES[start..end]))",
        "        unsafe {",
        "            ADDRESSES[index] = address",
        "        }",
        "        if address != 0 {",
        "            loaded += 1",
        "        }",
        "        start = end + 1",
        "        index += 1",
        "    }",
        "    return loaded",
        "}",
        "",
        "// Forget every address; commands panic until the next load().",
        "pub unsafe fn unload() {",
        "    index := 0usize",
        "    for index < COMMAND_COUNT {",
        "        unsafe {",
        "            ADDRESSES[index] = 0",
        "        }",
        "        index += 1",
        "    }",
        "}",
        "",
        "// Whether the command with this registry name (\"glDispatchCompute\") is",
        "// loaded. Unknown names are not.",
        "pub fn is_loaded(name: &str) -> bool {",
        "    wanted := mem.str_bytes(name)",
        "    start := 0usize",
        "    index := 0usize",
        "    for index < COMMAND_COUNT {",
        "        end := start",
        "        for NAMES[end] != 0 {",
        "            end += 1",
        "        }",
        "        if bytes.equal(&NAMES[start..end], wanted) {",
        "            return unsafe {",
        "                ADDRESSES[index]",
        "            } != 0",
        "        }",
        "        start = end + 1",
        "        index += 1",
        "    }",
        "    return false",
        "}",
        "",
        "// The debug message callback glDebugMessageCallback takes: source, type,",
        "// id, severity, message length, NUL-terminated message and user parameter.",
        f"pub type DebugProc = {DEBUG_PROC}",
        "",
        "// Enums",
        "",
    ]
    for name, (value, kind, _) in enums:
        constant = name[3:] if name.startswith("GL_") else name
        if constant[0].isdigit():
            constant = "N" + constant
        dodo = "u64" if kind == "ull" else "u32"
        if value.startswith("-"):
            raise SystemExit(f"{name}: negative value {value}")
        lines.append(f"pub const {constant}: {dodo} = {value.lower() if value.startswith('0x') else value}")
    lines += ["", "// Commands", ""]
    taken = set()
    for index, (name, number, command) in enumerate(commands):
        proto = command.find("proto")
        result = dodo_type(c_type(proto), name)
        function = identifier(name[2:])
        if function in taken:
            raise SystemExit(f"{name}: duplicate Dodo name {function}")
        taken.add(function)
        params = []
        for param in command.findall("param"):
            pname = identifier(param.find("name").text)
            params.append((pname, dodo_type(c_type(param), f"{name}.{pname}")))
        signature = ", ".join(f"{p}: {t}" for p, t in params)
        types = ", ".join(t for _, t in params)
        returns = "" if result == "void" else f" -> {result}"
        args = ", ".join(p for p, _ in params)
        lines.append(f"// {name}, OpenGL {number[0]}.{number[1]}.")
        lines.append(f"pub unsafe fn {function}({signature}){returns} {{")
        lines.append("    unsafe {")
        lines.append(f"        address := ADDRESSES[{index}]")
        lines.append(f"        core.assert(address != 0) // {name} is not loaded")
        lines.append(f'        call := address as extern "C" fn({types}){returns}')
        if result == "void":
            lines.append(f"        call({args})")
        else:
            lines.append(f"        return call({args})")
        lines.append("    }")
        lines.append("}")
    return "\n".join(lines) + "\n"


def fetch(directory: Path) -> Path:
    """Download the pinned registry and verify it."""
    data = urllib.request.urlopen(URL, timeout=60).read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != SHA256:
        raise SystemExit(f"{URL}: SHA-256 {digest}, expected {SHA256}")
    path = directory / "gl.xml"
    path.write_bytes(data)
    return path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--registry", type=Path, help="local gl.xml instead of the pinned download")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="dodo-gl-registry-") as directory:
        text = render(args.registry or fetch(Path(directory)))
    if args.write:
        TARGET.parent.mkdir(parents=True, exist_ok=True)
        TARGET.write_text(text)
        return 0
    if not TARGET.exists() or TARGET.read_text() != text:
        print(f"{TARGET.relative_to(ROOT)} is out of date; run with --write", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
