---
title: "GPU programming with Vulkan"
description: "Call Vulkan directly from Dodo with std/gpu/vulkan: raw bindings generated from the Khronos registry, loaded at run time, with no C code."
section: "Standard library"
order: 160.7
---

`std/gpu/vulkan` is the Vulkan API, one to one, written entirely in Dodo. It
is the lowest layer of Dodo's GPU support, in the same role as Rust's `ash`:
a future portable `std/gpu` will be built on it, and you can use it directly
today for compute and graphics. The bindings are generated from the Khronos
registry (`vk.xml`), so names, fields and values follow the
[Vulkan specification](https://registry.khronos.org/vulkan/specs/latest/html/).

Nothing is linked when you build. The package opens the Vulkan loader when
your program calls `vk.open()`, so the same executable starts on machines
without Vulkan and can report it:

| System | Library opened | Driver |
| --- | --- | --- |
| Linux (x86-64, AArch64) | `libvulkan.so.1` | Your GPU's Mesa or vendor driver; Mesa's lavapipe runs on the CPU |
| Windows (x64) | `vulkan-1.dll` | Installed with every GPU driver |
| macOS (Apple silicon, Intel) | `libvulkan.1.dylib`, then `libMoltenVK.dylib` | MoltenVK, from the Vulkan SDK or `brew install molten-vk vulkan-loader` |

Bound: core Vulkan 1.0 to 1.4, surfaces for Xlib, XCB, Wayland, Win32, Metal
and headless use, swapchains, portability enumeration and subset (MoltenVK),
and debug utils.

## Quickstart

[`examples/vulkan_compute.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/vulkan_compute.dodo)
runs a compute shader that updates 1000 values in a storage buffer and checks
the results:

```sh
dodo run examples/vulkan_compute.dodo
```

```text
device: llvmpipe (LLVM 19.1.7, 128 bits)
1000 of 1000 values correct
compute ok
```

It exits 0 on success, 2 with `NotFound` when there is no Vulkan loader, and
1 with the failing Result's name (such as `ERROR_INCOMPATIBLE_DRIVER` when a
loader has no driver).

## Opening Vulkan

```dodo
package main

import "core/ptr"
import "std/console"
import "std/gpu/vulkan" as vk

fn main() -> i32 {
    library := match vk.open() {
        ok(library) => library,
        err(error) => {
            console.println(vk.load_error_name(error))!
            return 2
        }
    }
    entry := library.entry()

    app := vk.ApplicationInfo.default()
    app.p_application_name = ptr.as_ptr(b"hello\0")
    app.api_version = vk.API_VERSION_1_3
    info := vk.InstanceCreateInfo.default()
    info.p_application_info = ptr.from_ref(&app)

    instance := vk.null_handle::<vk.InstanceT>()
    created := unsafe {
        entry.create_instance(ptr.from_ref(&info), vk.no_allocator(), ptr.from_mut(&mut instance))
    }
    if created != vk.SUCCESS {
        console.println(vk.result_name(created))!
        return 1
    }
    fns := unsafe { library.instance(instance) }
    count := 0u32
    unsafe {
        fns.enumerate_physical_devices(instance, ptr.from_mut(&mut count), 0usize as *mut vk.PhysicalDevice)
        fns.destroy_instance(instance, vk.no_allocator())
    }
    console.printf("{} devices\n", count)!
    return 0
}
```

Commands live in three tables of `extern "C" fn` pointers, filled through
`vkGetInstanceProcAddr` and `vkGetDeviceProcAddr`:

| Table | Obtained with | Holds |
| --- | --- | --- |
| `vk.EntryFns` | `library.entry()` | `create_instance` and the instance-level queries |
| `vk.InstanceFns` | `library.instance(instance)` | Instance and physical-device commands, `create_device` |
| `vk.DeviceFns` | `vk.device_fns(&instance_fns, device)` | Device, queue and command-buffer commands |

A command the driver lacks, or whose extension or API version you did not
enable, holds address zero instead of a callable pointer. Check
`unsafe { fns.cmd_begin_rendering as usize } != 0` before calling an optional
one. `vk.open_path(b"/path/libMoltenVK.dylib\0")`
opens a specific loader or driver instead of the system's.

## Names and types

The C names lose their `Vk`, `VK_` and `vk` prefixes; fields and commands
become snake_case, splitting words as Rust's `ash` does:

| C | Dodo |
| --- | --- |
| `VkInstanceCreateInfo` | `vk.InstanceCreateInfo` |
| `ppEnabledExtensionNames` | `pp_enabled_extension_names` |
| `maxImageDimension2D` | `max_image_dimension2_d` |
| `VK_FORMAT_R8G8B8A8_UNORM` | `vk.FORMAT_R8G8B8A8_UNORM` |
| `vkCmdDispatch` | `device_fns.cmd_dispatch` |
| `PFN_vkCmdDispatch` | `vk.PfnCmdDispatch` |
| `VK_KHR_SWAPCHAIN_EXTENSION_NAME` | `vk.KHR_SWAPCHAIN_EXTENSION_NAME` (`&[u8]`, NUL-terminated) |

A field or parameter that is a Dodo keyword gets a trailing underscore:
`VkDescriptorPoolSize.type` is `type_`.

| C kind | Dodo representation |
| --- | --- |
| Enum (`VkFormat`) | `i32` alias with one constant per value |
| Flags and flag bits | `u32` alias, or `u64` for the 64-bit `*Flags2` types |
| Dispatchable handle (`VkDevice`) | `*mut vk.DeviceT`; null is `vk.null_handle::<vk.DeviceT>()` |
| Non-dispatchable handle (`VkBuffer`) | `u64`; null is `vk.NULL_HANDLE` |
| Struct | `@repr(C)` struct with the same layout, and `default()` |
| Union (`VkClearValue`) | `@repr(C)` struct with one word array, `data` |
| Callback (`PFN_vkDebugUtilsMessengerCallbackEXT`) | `extern "C" fn` alias |
| `const char*`, `void*` | `*const u8`, `*mut u8` |

Every struct's `default()` returns all fields zero with `s_type` already set,
so you assign only what matters. All-zero is valid for every Vulkan struct;
`unsafe { vk.zeroed::<[16]vk.QueueFamilyProperties>() }` makes an array of
them for a command to fill in, since Dodo cannot repeat a struct value in an
array literal.

Union constructors fill the right member: `vk.clear_color_float`,
`vk.clear_color_int`, `vk.clear_color_uint`, `vk.clear_value_color` and
`vk.clear_value_depth_stencil`.

## Results and helpers

| Call | Returns |
| --- | --- |
| `vk.check(code)` | `vk.Result!vk.Error`: non-negative codes succeed, negative ones fail |
| `vk.result_name(code)` | The code's name, such as `"ERROR_DEVICE_LOST"` |
| `vk.make_api_version(variant, major, minor, patch)` and `vk.api_version_major(v)` etc. | Packed versions |
| `vk.c_name(&props.extension_name)` | A fixed NUL-padded name field without its padding |
| `vk.name_eq(&props.extension_name, vk.KHR_SWAPCHAIN_EXTENSION_NAME)` | Whether the field holds that name |
| `vk.no_allocator()` | The null `pAllocator` argument |

Inside a function returning `void!vk.Error`, write
`vk.check(d.create_buffer(...))?` to propagate failures.

## Safety

Every command call is `unsafe`, as in C: Dodo does not check Vulkan's
valid-usage rules. Run with the Khronos validation layer
(`VK_LAYER_KHRONOS_validation`, enabled by `VK_INSTANCE_LAYERS` or in the
instance create info) while developing. Pointers stored in create infos are
raw: keep the pointed-to values alive until the command returns, and binding
them to locals first, as the examples do, guarantees it.

## Shaders

Vulkan takes SPIR-V. Compile GLSL or HLSL with `glslangValidator -V` or
`glslc`, and pass the words in a `[N]u32` constant, as the example does.

## Platform notes

On macOS, enumerate instance extensions and, when
`VK_KHR_portability_enumeration` is offered, enable it and set
`INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR`; the loader hides MoltenVK
otherwise. The compute example shows this. Windows (x64) uses the same C
calling convention as everything else in Dodo, so 32-bit `stdcall` concerns
do not arise.

## Updating the bindings

`scripts/generate_vulkan.py path/to/vk.xml` regenerates `types.dodo` and
`commands.dodo` from a registry in
[Vulkan-Headers](https://github.com/KhronosGroup/Vulkan-Headers); add an
extension to its `EXTENSIONS` list to bind it.
`scripts/vulkan_layout_check.py path/to/vk.xml DIR` writes a C program and a
Dodo program that print every struct's size, alignment and field offsets;
their outputs must be identical.
