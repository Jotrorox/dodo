#!/usr/bin/env python3
"""Generate std/gpu/vulkan's raw bindings from the Khronos Vulkan registry.

Usage: generate_vulkan.py path/to/vk.xml

vk.xml is published in https://github.com/KhronosGroup/Vulkan-Headers under
registry/. The script writes stdlib/std/gpu/vulkan/types.dodo (constants,
handles, enums, bitmasks, structs) and commands.dodo (function pointer types
and the three dispatch tables with their loaders). The hand-written loader in
stdlib/std/gpu/vulkan/loader.dodo opens the Vulkan library and fills them.

The bindings cover the core API up to MAX_VERSION plus the extensions in
EXTENSIONS: the window-system surfaces, swapchains, portability (MoltenVK)
and debug utils. Add an extension name there and rerun to bind another one.

Mapping, kept close to the C API so its documentation applies directly:
  VkFormat, VK_FORMAT_R8_UNORM      -> Format (an i32 alias), FORMAT_R8_UNORM
  VkBufferUsageFlags                -> BufferUsageFlags (a u32 alias)
  VkInstanceCreateInfo.sType        -> InstanceCreateInfo.s_type
  VkInstance (dispatchable handle)  -> Instance = *mut InstanceT
  VkBuffer (non-dispatchable)       -> Buffer = u64
  vkCreateBuffer                    -> DeviceFns.create_buffer: PfnCreateBuffer
  unions                            -> a struct with one word array, `data`
"""

import os
import re
import sys
import xml.etree.ElementTree as ET

MAX_VERSION = (1, 4)

EXTENSIONS = [
    "VK_KHR_surface",
    "VK_KHR_swapchain",
    "VK_KHR_xlib_surface",
    "VK_KHR_xcb_surface",
    "VK_KHR_wayland_surface",
    "VK_KHR_win32_surface",
    "VK_EXT_metal_surface",
    "VK_EXT_headless_surface",
    "VK_KHR_portability_enumeration",
    "VK_KHR_portability_subset",
    "VK_EXT_debug_utils",
]

API = "vulkan"
POINTER_SIZE = 8  # Every hosted Dodo target is 64-bit.

PRIMITIVES = {
    "void": "void",
    "char": "u8",
    "int8_t": "i8",
    "uint8_t": "u8",
    "int16_t": "i16",
    "uint16_t": "u16",
    "int32_t": "i32",
    "uint32_t": "u32",
    "int64_t": "i64",
    "uint64_t": "u64",
    "int": "i32",
    "size_t": "usize",
    "float": "f32",
    "double": "f64",
}

# Window-system types from platform headers. Opaque structs are only used
# behind pointers, so they become u8 pointees.
PLATFORM = {
    "Display": "u8",
    "VisualID": "u64",
    "Window": "u64",
    "xcb_connection_t": "u8",
    "xcb_window_t": "u32",
    "xcb_visualid_t": "u32",
    "wl_display": "u8",
    "wl_surface": "u8",
    "HINSTANCE": "*mut u8",
    "HWND": "*mut u8",
    "HMONITOR": "*mut u8",
    "HANDLE": "*mut u8",
    "DWORD": "u32",
    "LPCWSTR": "*const u16",
    "SECURITY_ATTRIBUTES": "u8",
    "CAMetalLayer": "u8",
}

SIZES = {
    "u8": 1, "i8": 1, "u16": 2, "i16": 2, "u32": 4, "i32": 4, "f32": 4,
    "u64": 8, "i64": 8, "f64": 8, "usize": 8,
}

DISPATCHABLE_INSTANCE = {"VkInstance", "VkPhysicalDevice"}
DISPATCHABLE_DEVICE = {"VkDevice", "VkQueue", "VkCommandBuffer"}
ENTRY_COMMANDS = {
    "vkGetInstanceProcAddr",
    "vkCreateInstance",
    "vkEnumerateInstanceExtensionProperties",
    "vkEnumerateInstanceLayerProperties",
    "vkEnumerateInstanceVersion",
}
RESERVED = {
    "package", "import", "pub", "fn", "struct", "enum", "interface", "type",
    "dyn", "const", "let", "return", "if", "else", "for", "in", "break",
    "continue", "match", "unsafe", "extern", "as", "from", "stores",
    "requires_plain", "static", "void", "mut", "true", "false", "asm",
    "self", "Self", "ok", "err", "some", "none",
}


def snake(name):
    """camelCase to snake_case, splitting acronyms like heck does."""
    out = []
    for i, c in enumerate(name):
        if i > 0 and c.isupper():
            prev = name[i - 1]
            nxt = name[i + 1] if i + 1 < len(name) else ""
            if prev.islower() or prev.isdigit() or (prev.isupper() and nxt.islower()):
                out.append("_")
        out.append(c.lower())
    result = "".join(out)
    return result + "_" if result in RESERVED else result


def strip_vk(name):
    if name.startswith("Vk"):
        return name[2:]
    if name.startswith("PFN_vk"):
        return "Pfn" + name[6:]
    raise ValueError(name)


def constant_name(name):
    assert name.startswith("VK_"), name
    return name[3:]


def command_field(name):
    assert name.startswith("vk"), name
    return snake(name[2:])


def api_ok(element):
    api = element.get("api")
    return api is None or API in api.split(",")


def version_tuple(text):
    major, minor = text.split(".")
    return int(major), int(minor)


class Registry:
    def __init__(self, path):
        self.root = ET.parse(path).getroot()
        self.types = {}
        for t in self.root.find("types"):
            if t.tag != "type" or not api_ok(t):
                continue
            name = t.get("name") or (t.findtext("name") or "").strip()
            if not name and t.find("proto") is not None:
                name = t.find("proto").findtext("name")
            if name:
                self.types[name] = t
        self.enums = {}
        self.constants = {}
        for block in self.root.findall("enums"):
            if block.get("name") == "API Constants":
                for e in block.findall("enum"):
                    if api_ok(e):
                        self.constants[e.get("name")] = e
            else:
                self.enums[block.get("name")] = block
        self.commands = {}
        for c in self.root.find("commands"):
            if not api_ok(c):
                continue
            proto = c.find("proto")
            name = c.get("name") if proto is None else proto.findtext("name")
            self.commands[name] = c
        self.extensions = {e.get("name"): e for e in self.root.find("extensions")}
        self.header_version = None
        for t in self.root.find("types"):
            if (
                t.get("category") == "define"
                and t.findtext("name") == "VK_HEADER_VERSION"
                and api_ok(t)
            ):
                self.header_version = int("".join(t.itertext()).split()[-1])

        self.selected = set()
        self.required_types = []
        self.required_commands = []
        self.required_enums = []  # (element, extension number or None)
        self.extension_constants = []

    def satisfied(self, expression):
        """Evaluate a registry `depends` expression against the selection."""
        tokens = re.findall(r"[A-Za-z0-9_:]+|[+,()]", expression)
        position = 0

        def primary():
            nonlocal position
            token = tokens[position]
            position += 1
            if token == "(":
                value = disjunction()
                position += 1
                return value
            return token.split("::")[0] in self.selected

        def conjunction():
            nonlocal position
            value = primary()
            while position < len(tokens) and tokens[position] == "+":
                position += 1
                value = primary() and value
            return value

        def disjunction():
            nonlocal position
            value = conjunction()
            while position < len(tokens) and tokens[position] == ",":
                position += 1
                value = conjunction() or value
            return value

        return disjunction()

    def select(self):
        features = [
            f for f in self.root.findall("feature")
            if api_ok(f) and version_tuple(f.get("number")) <= MAX_VERSION
        ]
        for f in features:
            self.selected.add(f.get("name"))
        for name in EXTENSIONS:
            self.selected.add(name)
        blocks = [(f, None) for f in features]
        for name in EXTENSIONS:
            extension = self.extensions[name]
            blocks.append((extension, int(extension.get("number"))))
        for block, number in blocks:
            for require in block.findall("require"):
                if not api_ok(require):
                    continue
                if require.get("depends") and not self.satisfied(require.get("depends")):
                    continue
                for item in require:
                    if not api_ok(item):
                        continue
                    if item.tag == "type":
                        self.required_types.append(item.get("name"))
                    elif item.tag == "command":
                        self.required_commands.append(item.get("name"))
                    elif item.tag == "enum":
                        if item.get("extends"):
                            self.required_enums.append((item, number))
                        elif number is not None and (
                            item.get("value") is not None or item.get("alias")
                        ):
                            self.extension_constants.append(item)
                        else:
                            self.required_types.append(item.get("name"))


class Generator:
    def __init__(self, registry):
        self.r = registry
        self.emitted_types = set()
        self.order = []
        self.handles = {}
        self.aliases = {}
        self.enum_values = {}  # enum type -> list of (name, value or alias)
        self.bitmask_width = {}
        self.layout_cache = {}

    # Dependency closure -------------------------------------------------

    def need(self, name):
        if name in self.emitted_types:
            return
        if name in PRIMITIVES or name in PLATFORM:
            return
        if name in self.r.constants:
            self.emitted_types.add(name)
            self.order.append(name)
            return
        element = self.r.types.get(name)
        if element is None:
            raise SystemExit(f"unknown type {name}")
        category = element.get("category")
        if category in ("include", "define"):
            return
        if category is None:
            raise SystemExit(f"unmapped external type {name}; add it to PLATFORM")
        self.emitted_types.add(name)
        if element.get("alias"):
            self.need(element.get("alias"))
            self.order.append(name)
            return
        if category in ("struct", "union"):
            for member in element.findall("member"):
                if not api_ok(member):
                    continue
                self.need(member.findtext("type"))
                size = member.findtext("enum")
                if size:
                    self.need(size)
        elif category == "funcpointer":
            self.need_signature(element)
        elif category == "bitmask":
            bits = element.get("requires") or element.get("bitvalues")
            if bits:
                self.need(bits)
        elif category == "basetype":
            inner = element.findtext("type")
            if inner:
                self.need(inner)
        self.order.append(name)

    def need_signature(self, element):
        self.need(element.find("proto").findtext("type"))
        for param in element.findall("param"):
            if api_ok(param):
                self.need(param.findtext("type"))

    # Type spelling -------------------------------------------------------

    def base(self, ctype):
        if ctype in PRIMITIVES:
            return PRIMITIVES[ctype]
        if ctype in PLATFORM:
            return PLATFORM[ctype]
        return strip_vk(ctype)

    def spell(self, element, parameter=False):
        """The Dodo type of a member or parameter element."""
        ctype = element.findtext("type")
        before = (element.text or "")
        after = element.find("type").tail or ""
        children = list(element)
        name_element = element.find("name")
        suffix = name_element.tail or ""
        for child in children[children.index(name_element) + 1:]:
            if child.tag == "enum":
                suffix += child.text or ""
            suffix += child.tail or ""
        if ":" in suffix:
            raise SystemExit(f"bitfield member in {ET.tostring(element)}")
        base_const = "const" in before
        ty = self.base(ctype)
        stars = after.split("*")
        if ty == "void" and len(stars) > 1:
            ty = "u8"

        for level in range(len(stars) - 1):
            pointee_const = base_const if level == 0 else "const" in stars[level]
            ty = f"*{'const' if pointee_const else 'mut'} {ty}"
        dims = re.findall(r"\[([^\]]+)\]", suffix)
        if dims:
            if parameter:
                ty = f"*{'const' if base_const else 'mut'} {ty}"
            else:
                for dim in reversed(dims):
                    dim = dim.strip()
                    count = dim if dim.isdigit() else str(self.constant_value(dim))
                    ty = f"[{count}]{ty}"
        return ty

    def constant_value(self, name):
        e = self.r.constants[name]
        if e.get("alias"):
            return self.constant_value(e.get("alias"))
        return int(e.get("value").rstrip("U"))

    # Layout (for unions only) -------------------------------------------

    def layout(self, ty):
        if ty.startswith("*"):
            return POINTER_SIZE, POINTER_SIZE
        if ty.startswith("["):
            count, inner = re.match(r"\[(\d+)\](.*)", ty).groups()
            size, align = self.layout(inner)
            return size * int(count), align
        if ty in SIZES:
            return SIZES[ty], SIZES[ty]
        return self.named_layout("Vk" + ty)

    def named_layout(self, cname):
        if cname in self.layout_cache:
            return self.layout_cache[cname]
        element = self.r.types[cname]
        category = element.get("category")
        if element.get("alias"):
            result = self.named_layout(element.get("alias"))
        elif category in ("struct", "union"):
            size = 0
            align = 1
            for member in element.findall("member"):
                if not api_ok(member):
                    continue
                msize, malign = self.layout(self.spell(member))
                align = max(align, malign)
                if category == "struct":
                    size = (size + malign - 1) // malign * malign + msize
                else:
                    size = max(size, msize)
            size = (size + align - 1) // align * align
            result = (size, align)
        elif category == "handle":
            result = (8, 8)
        elif category in ("enum",):
            result = (4, 4)
        elif category in ("bitmask", "basetype"):
            inner = self.alias_target(cname)
            result = self.layout(inner)
        elif category == "funcpointer":
            result = (POINTER_SIZE, POINTER_SIZE)
        else:
            raise SystemExit(f"no layout for {cname}")
        self.layout_cache[cname] = result
        return result

    def alias_target(self, cname):
        element = self.r.types[cname]
        inner = element.findtext("type")
        if element.get("category") == "bitmask":
            return "u64" if inner == "VkFlags64" else "u32"
        return self.base(inner)

    # Enum values ----------------------------------------------------------

    def collect_enums(self):
        for name in self.order:
            element = self.r.types.get(name)
            if element is None or element.get("category") != "enum" or element.get("alias"):
                continue
            block = self.r.enums.get(name)
            values = []
            if block is not None:
                for e in block.findall("enum"):
                    if not api_ok(e):
                        continue
                    values.append((e.get("name"), self.enum_value(e, None)))
                if block.get("bitwidth") == "64":
                    self.bitmask_width[name] = 64
            self.enum_values[name] = values
        seen = {n for vs in self.enum_values.values() for n, _ in vs}
        for item, number in self.r.required_enums:
            target = item.get("extends")
            if item.get("name") in seen or target not in self.enum_values:
                continue
            seen.add(item.get("name"))
            self.enum_values[target].append((item.get("name"), self.enum_value(item, number)))

    def enum_value(self, e, number):
        if e.get("alias"):
            return ("alias", e.get("alias"))
        if e.get("bitpos") is not None:
            return 1 << int(e.get("bitpos"))
        if e.get("value") is not None:
            text = e.get("value")
            return int(text, 16) if text.lower().startswith(("0x", "-0x")) else int(text)
        if e.get("offset") is not None:
            extension = int(e.get("extnumber") or number)
            value = 1000000000 + (extension - 1) * 1000 + int(e.get("offset"))
            return -value if e.get("dir") == "-" else value
        raise SystemExit(f"enum without value {e.get('name')}")

    # Emission --------------------------------------------------------------

    def run(self):
        for name in self.r.required_types:
            self.need(name)
        for name in self.r.required_commands:
            command = self.r.commands[name]
            if command.get("alias"):
                continue
            self.need_signature(command)
        self.collect_enums()
        return self.types_file(), self.commands_file()

    def header(self, what):
        return (
            "package vulkan\n\n"
            f"// {what}\n"
            "//\n"
            f"// Generated by scripts/generate_vulkan.py from vk.xml (header version\n"
            f"// {self.r.header_version}); do not edit. Core Vulkan up to "
            f"{MAX_VERSION[0]}.{MAX_VERSION[1]} and the extensions\n"
            "// listed in the script are bound.\n\n"
        )

    def types_file(self):
        out = [self.header("Vulkan constants, handles, enums, bitmasks and structs.")]
        major, minor = MAX_VERSION
        out.append(f"pub const HEADER_VERSION: u32 = {self.r.header_version}\n")
        for v in range(0, minor + 1):
            out.append(f"pub const API_VERSION_{major}_{v}: u32 = {(major << 22) | (v << 12)}\n")
        out.append("\n")
        constants = [n for n in self.order if n in self.r.constants]
        for name in constants:
            out.append(self.api_constant(name))
        out.append("\n")
        for item in self.r.extension_constants:
            out.append(self.extension_constant(item))
        out.append("\n")
        for name in self.order:
            if name in self.r.constants:
                continue
            element = self.r.types[name]
            category = element.get("category")
            if element.get("alias"):
                out.append(f"pub type {strip_vk(name)} = {strip_vk(element.get('alias'))}\n")
            elif category == "basetype":
                inner = element.findtext("type")
                if inner is None:
                    out.append(f"pub struct {strip_vk(name)} {{\n    opaque: u8\n}}\n")
                else:
                    out.append(f"pub type {strip_vk(name)} = {self.base(inner)}\n")
            elif category == "bitmask":
                out.append(f"pub type {strip_vk(name)} = {self.alias_target(name)}\n")
            elif category == "handle":
                out.append(self.handle(name, element))
            elif category == "enum":
                out.append(self.enum(name))
            elif category in ("struct", "union"):
                out.append(self.struct(name, element))
            elif category == "funcpointer":
                out.append(self.funcpointer(name, element))
            else:
                raise SystemExit(f"unhandled category {category} for {name}")
        out.append(self.result_names())
        return "".join(out)

    def api_constant(self, name):
        e = self.r.constants[name]
        if e.get("alias"):
            return f"pub const {constant_name(name)}: {self.constant_type(e.get('alias'))} = {constant_name(e.get('alias'))}\n"
        ty = self.constant_type(name)
        value = e.get("value")
        if value.startswith("(~"):
            body = value.strip("()~").rstrip("ULF")
            bits = 64 if ty == "u64" else 32
            value = str((~int(body)) & ((1 << bits) - 1))
        elif ty == "f32":
            value = value.rstrip("F")
        else:
            value = value.rstrip("U")
        return f"pub const {constant_name(name)}: {ty} = {value}\n"

    def constant_type(self, name):
        e = self.r.constants[name]
        if e.get("alias"):
            return self.constant_type(e.get("alias"))
        return {"uint32_t": "u32", "uint64_t": "u64", "float": "f32", "int32_t": "i32"}[e.get("type")]

    def extension_constant(self, item):
        name = item.get("name")
        if item.get("alias"):
            return ""
        value = item.get("value")
        if value.startswith('"'):
            return f'pub const {constant_name(name)}: &[u8] = b"{value.strip(chr(34))}\\0"\n'
        return f"pub const {constant_name(name)}: u32 = {value}\n"

    def handle(self, name, element):
        dispatchable = element.findtext("type") == "VK_DEFINE_HANDLE"
        short = strip_vk(name)
        if dispatchable:
            return (
                f"// An opaque driver object behind the dispatchable handle {short}.\n"
                f"pub struct {short}T {{\n    opaque: u8\n}}\n"
                f"pub type {short} = *mut {short}T\n"
            )
        return f"pub type {short} = u64\n"

    def enum(self, name):
        short = strip_vk(name)
        block = self.r.enums.get(name)
        is_bits = block.get("type") == "bitmask" if block is not None else "FlagBits" in name
        width = self.bitmask_width.get(name, 32)
        if is_bits:
            base = "u64" if width == 64 else "u32"
        else:
            base = "i32"
        out = [f"pub type {short} = {base}\n"]
        for value_name, value in self.enum_values.get(name, []):
            if isinstance(value, tuple):
                target = value[1]
                if any(n == target for n, _ in self.enum_values[name]):
                    out.append(f"pub const {constant_name(value_name)}: {short} = {constant_name(target)}\n")
                continue
            if is_bits and value < 0:
                value &= (1 << width) - 1
            text = hex(value) if is_bits and value > 9 else str(value)
            out.append(f"pub const {constant_name(value_name)}: {short} = {text}\n")
        return "".join(out)

    def struct(self, name, element):
        short = strip_vk(name)
        lines = []
        if element.get("category") == "union":
            size, align = self.named_layout(name)
            word = "u64" if align == 8 else "u32"
            count = size // (8 if align == 8 else 4)
            members = ", ".join(m.findtext("name") for m in element.findall("member") if api_ok(m))
            lines.append(f"// The C union {name} of {members}.\n")
            lines.append(f"@repr(C)\npub struct {short} {{\n    pub data: [{count}]{word}\n")
        else:
            lines.append(f"@repr(C)\npub struct {short} {{\n")
            for member in element.findall("member"):
                if not api_ok(member):
                    continue
                lines.append(f"    pub {snake(member.findtext('name'))}: {self.spell(member)}\n")
        s_type = None
        for member in element.findall("member"):
            if member.findtext("name") == "sType" and member.get("values"):
                s_type = member.get("values").split(",")[0]
        lines.append(f"\n    // All zero{', with s_type set' if s_type else ''}.\n")
        lines.append(f"    pub fn default() -> {short} {{\n")
        if s_type:
            lines.append(f"        value := unsafe {{\n            zeroed::<{short}>()\n        }}\n")
            lines.append(f"        value.s_type = {constant_name(s_type)}\n")
            lines.append("        return value\n")
        else:
            lines.append(f"        return unsafe {{\n            zeroed::<{short}>()\n        }}\n")
        lines.append("    }\n}\n")
        return "".join(lines)

    def signature(self, element):
        proto = element.find("proto")
        ret = self.spell(proto)
        params = [self.spell(p, parameter=True) for p in element.findall("param") if api_ok(p)]
        text = f'extern "C" fn({", ".join(params)})'
        if ret != "void":
            text += f" -> {ret}"
        return text

    def funcpointer(self, name, element):
        return f"pub type {strip_vk(name)} = {self.signature(element)}\n"

    def result_names(self):
        out = [
            "\n// The name of a Result code, such as \"ERROR_DEVICE_LOST\", or \"UNKNOWN\".\n",
            "pub fn result_name(code: Result) -> &str from(static) {\n",
        ]
        for value_name, value in self.enum_values["VkResult"]:
            if isinstance(value, tuple):
                continue
            out.append(f"    if code == {constant_name(value_name)} {{\n")
            out.append(f'        return "{constant_name(value_name)}"\n    }}\n')
        out.append('    return "UNKNOWN"\n}\n')
        return "".join(out)

    def commands_file(self):
        out = [self.header("Vulkan command signatures and dispatch tables.")]
        tables = {"entry": [], "instance": [], "device": []}
        seen = set()
        for name in self.r.required_commands:
            command = self.r.commands[name]
            if command.get("alias") or name in seen:
                continue
            seen.add(name)
            first = command.find("param").findtext("type")
            if name in ENTRY_COMMANDS:
                table = "entry"
            elif name == "vkGetDeviceProcAddr" or first in DISPATCHABLE_INSTANCE:
                table = "instance"
            elif first in DISPATCHABLE_DEVICE:
                table = "device"
            else:
                raise SystemExit(f"cannot dispatch {name}")
            tables[table].append(name)
            out.append(f"pub type Pfn{name[2:]} = {self.signature(command)}\n")
        out.append("\n")
        docs = {
            "entry": (
                "EntryFns",
                "Commands available before an instance exists, resolved with\n"
                "// vkGetInstanceProcAddr(NULL, name).",
                "get_instance_proc_addr: PfnGetInstanceProcAddr",
                "get_instance_proc_addr",
                "null_handle::<InstanceT>()",
            ),
            "instance": (
                "InstanceFns",
                "Instance and physical-device commands of one instance, resolved\n"
                "// with vkGetInstanceProcAddr(instance, name).",
                "get_instance_proc_addr: PfnGetInstanceProcAddr, instance: Instance",
                "get_instance_proc_addr",
                "instance",
            ),
            "device": (
                "DeviceFns",
                "Device, queue and command-buffer commands of one device, resolved\n"
                "// with vkGetDeviceProcAddr(device, name).",
                "get_device_proc_addr: PfnGetDeviceProcAddr, device: Device",
                "get_device_proc_addr",
                "device",
            ),
        }
        for table, (struct, doc, params, getter, handle) in docs.items():
            out.append(
                f"// {doc} A command the driver does not\n"
                "// provide, or whose extension or version was not enabled, holds address\n"
                "// zero: check `unsafe { fns.name as usize } != 0` before calling it.\n"
            )
            out.append(f"pub struct {struct} {{\n")
            for name in tables[table]:
                out.append(f"    pub {command_field(name)}: Pfn{name[2:]}\n")
            out.append("}\n\n")
            out.append(f"// Resolve every {struct} command. Unsafe: the getter must be the\n")
            out.append("// loader's, and the handle live for as long as the table is used.\n")
            out.append(f"pub unsafe fn load_{table}({params}) -> {struct} {{\n")
            out.append("    unsafe {\n")
            out.append(f"        fns := zeroed::<{struct}>()\n")
            for name in tables[table]:
                field = command_field(name)
                if name == "vkGetInstanceProcAddr":
                    out.append(f"        fns.{field} = get_instance_proc_addr\n")
                    continue
                out.append(
                    f"        fns.{field} = {getter}({handle}, ptr.as_ptr(b\"{name}\\0\")) as Pfn{name[2:]}\n"
                )
            out.append("        return fns\n    }\n}\n\n")
        text = "".join(out).rstrip("\n") + "\n"
        return text.replace("package vulkan\n\n", "package vulkan\n\nimport \"core/ptr\"\n\n", 1)


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    registry = Registry(sys.argv[1])
    registry.select()
    types_text, commands_text = Generator(registry).run()
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "stdlib", "std", "gpu", "vulkan")
    os.makedirs(root, exist_ok=True)
    for name, text in (("types.dodo", types_text), ("commands.dodo", commands_text)):
        with open(os.path.join(root, name), "w", newline="\n") as f:
            f.write(text)
        print(f"wrote {os.path.normpath(os.path.join(root, name))}: {text.count(chr(10))} lines")


if __name__ == "__main__":
    main()
