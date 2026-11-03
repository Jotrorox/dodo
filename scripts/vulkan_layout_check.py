#!/usr/bin/env python3
"""Cross-check std/gpu/vulkan's struct layouts against the C headers.

Usage: vulkan_layout_check.py path/to/vk.xml OUTPUT_DIRECTORY

Writes layout.c and layout.dodo to OUTPUT_DIRECTORY. Each prints the size and
alignment of every generated struct and the offset of every field, one per
line, in the same order and format. Compile layout.c against the Vulkan
headers (`cc layout.c -o layout`, with libvulkan-dev or the Vulkan SDK) and
run `dodo run layout.dodo` for the same architecture; the two outputs must be
identical. Window-system structs whose headers are rarely installed (Xlib,
XCB, Wayland, Win32, Metal) are left out; their fields are all pointers and
32-bit values.

The structs and their Dodo field names are read from the checked-in
stdlib/std/gpu/vulkan/types.dodo (written by scripts/gen/vulkan.dodo), and
the C field names from vk.xml, which must be the registry types.dodo was
generated from.
"""

import os
import re
import sys
import xml.etree.ElementTree as ET

API = "vulkan"
TYPES = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "stdlib", "std", "gpu", "vulkan", "types.dodo"
)

PLATFORM_STRUCTS = {
    "VkXlibSurfaceCreateInfoKHR",
    "VkXcbSurfaceCreateInfoKHR",
    "VkWaylandSurfaceCreateInfoKHR",
    "VkWin32SurfaceCreateInfoKHR",
    "VkMetalSurfaceCreateInfoEXT",
}


def api_ok(element):
    api = element.get("api")
    return api is None or API in api.split(",")


def registry_types(path):
    """vk.xml's type elements by name, as scripts/gen/vulkan.dodo indexes them."""
    types = {}
    for t in ET.parse(path).getroot().find("types"):
        if t.tag != "type" or not api_ok(t):
            continue
        name = t.get("name") or (t.findtext("name") or "").strip()
        if name:
            types[name] = t
    return types


def generated_structs():
    """(name, [field, ...]) of every @repr(C) struct in types.dodo, in order."""
    structs = []
    with open(TYPES) as f:
        lines = f.read().split("\n")
    for index, line in enumerate(lines):
        if line != "@repr(C)":
            continue
        name = re.fullmatch(r"pub struct (\w+) \{", lines[index + 1]).group(1)
        fields = []
        for body in lines[index + 2:]:
            field = re.fullmatch(r"    pub (\w+): .*", body)
            if not field:
                break
            fields.append(field.group(1))
        structs.append((name, fields))
    return structs


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    types = registry_types(sys.argv[1])
    c = [
        "#define VK_ENABLE_BETA_EXTENSIONS",
        "#include <vulkan/vulkan.h>",
        "#include <stddef.h>",
        "#include <stdio.h>",
        "",
        "int main(void) {",
    ]
    dodo = [
        "package main",
        "",
        'import "core/mem"',
        'import "std/console"',
        'import "std/gpu/vulkan" as vk',
        "",
        "fn main() {",
    ]
    for short, fields in generated_structs():
        name = "Vk" + short
        element = types.get(name)
        if element is None:
            raise SystemExit(f"{name} is not in {sys.argv[1]}; regenerate types.dodo from it")
        if name in PLATFORM_STRUCTS:
            continue
        c.append(f'    printf("{short} %zu %zu\\n", sizeof({name}), _Alignof({name}));')
        dodo.append(
            f'    console.printf("{short} {{}} {{}}\\n", '
            f"mem.size_of::<vk.{short}>(), mem.align_of::<vk.{short}>())!"
        )
        if element.get("category") == "union":
            continue
        members = [m.findtext("name") for m in element.findall("member") if api_ok(m)]
        if len(members) != len(fields):
            raise SystemExit(f"{name}: types.dodo does not match {sys.argv[1]}; regenerate it")
        for c_field, field in zip(members, fields):
            c.append(f'    printf("{short}.{field} %zu\\n", offsetof({name}, {c_field}));')
            dodo.append(
                f'    console.printf("{short}.{field} {{}}\\n", '
                f'mem.offset_of::<vk.{short}>("{field}"))!'
            )
    c += ["    return 0;", "}", ""]
    dodo += ["}", ""]
    os.makedirs(sys.argv[2], exist_ok=True)
    for file, lines in (("layout.c", c), ("layout.dodo", dodo)):
        with open(os.path.join(sys.argv[2], file), "w", newline="\n") as f:
            f.write("\n".join(lines))
    print(f"wrote layout.c and layout.dodo to {sys.argv[2]}")


if __name__ == "__main__":
    main()
