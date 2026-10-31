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
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generate_vulkan as g  # noqa: E402

PLATFORM_STRUCTS = {
    "VkXlibSurfaceCreateInfoKHR",
    "VkXcbSurfaceCreateInfoKHR",
    "VkWaylandSurfaceCreateInfoKHR",
    "VkWin32SurfaceCreateInfoKHR",
    "VkMetalSurfaceCreateInfoEXT",
}


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    registry = g.Registry(sys.argv[1])
    registry.select()
    generator = g.Generator(registry)
    generator.run()
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
    for name in generator.order:
        element = registry.types.get(name)
        if (
            element is None
            or element.get("category") not in ("struct", "union")
            or element.get("alias")
            or name in PLATFORM_STRUCTS
        ):
            continue
        short = g.strip_vk(name)
        c.append(f'    printf("{short} %zu %zu\\n", sizeof({name}), _Alignof({name}));')
        dodo.append(
            f'    console.printf("{short} {{}} {{}}\\n", '
            f"mem.size_of::<vk.{short}>(), mem.align_of::<vk.{short}>())!"
        )
        if element.get("category") == "union":
            continue
        for member in element.findall("member"):
            if not g.api_ok(member):
                continue
            c_field = member.findtext("name")
            field = g.snake(c_field)
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
