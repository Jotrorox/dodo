---
title: "GPU programming"
description: "Compute and render on the GPU with std/gpu: one API over Metal, Vulkan and OpenGL, with checked buffers, textures, pipelines and passes."
section: "Standard library"
order: 160.65
---

`std/gpu` runs compute shaders and draws on the GPU through one API, whatever
graphics interface the machine has. It follows WebGPU's model: a device
creates buffers, textures, samplers and pipelines, and a command encoder
records compute passes, render passes and copies that `submit()` runs in
order. Every call is checked before it reaches the GPU, and objects stay alive
until the GPU has finished with them, so safe code cannot make the GPU read
freed memory or copy outside a buffer.

It is written in Dodo on top of the raw bindings, which stay available for
everything it does not cover:

| System | Backends, in the order `Device.open` tries them | Needs |
| --- | --- | --- |
| macOS (Apple silicon, Intel) | [Metal](metal.md), then [Vulkan](vulkan.md) through MoltenVK | Nothing to install |
| Linux (x86-64, AArch64) | [Vulkan](vulkan.md), then [OpenGL](opengl.md) | A Vulkan 1.2 or OpenGL 4.5 driver; Mesa's lavapipe and llvmpipe run on the CPU |
| Windows (x64) | [Vulkan](vulkan.md), then [OpenGL](opengl.md) | Any current GPU driver |

Nothing but Metal is linked. Vulkan and OpenGL are opened when a device is
created, so a program starts on any machine and gets an error it can handle
when there is no usable GPU.

## Quickstart

[`examples/gpu_compute.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/gpu_compute.dodo)
computes `y = a * x + y` for 1000 values and checks the results:

```sh
dodo run examples/gpu_compute.dodo
```

```text
Metal: Apple M2
1000 of 1000 values correct
compute ok
```

On Linux the first line reads `Vulkan: llvmpipe (LLVM 19.1.7, 128 bits)` or
similar. Without a usable GPU it prints `NoBackend` and exits 2. The heart of
the program, with the shader sources left out:

```dodo
fn run(device: &gpu.Device) -> u32!gpu.Error {
    xs := [0.0f32; 1000]
    ys := [0.0f32; 1000]
    for i in 0usize..1000 {
        xs[i] = i as f32
        ys[i] = 1.0
    }
    a := [2.5f32]
    count := [1000u32]
    params := device.create_buffer(16, gpu.BUFFER_UNIFORM | gpu.BUFFER_COPY_DST)?
    device.write_buffer(&params, 0, gpu.f32_bytes(&a))?
    device.write_buffer(&params, 4, gpu.u32_bytes(&count))?
    x := device.create_buffer_with_data(gpu.f32_bytes(&xs), gpu.BUFFER_STORAGE)?
    y := device.create_buffer_with_data(gpu.f32_bytes(&ys), gpu.BUFFER_STORAGE | gpu.BUFFER_COPY_SRC)?

    // The kernel in each backend's language.
    source := gpu.ShaderSource { spirv: &SPIRV, msl: MSL, glsl: GLSL }
    bindings := [
        gpu.BindingLayout.uniform_buffer(0, gpu.STAGE_COMPUTE),
        gpu.BindingLayout.read_only_storage_buffer(1, gpu.STAGE_COMPUTE),
        gpu.BindingLayout.storage_buffer(2, gpu.STAGE_COMPUTE),
    ]
    descriptor := gpu.ComputePipelineDescriptor.new(&source, "saxpy", 64, 1, 1)
    descriptor.bindings = &bindings
    pipeline := device.create_compute_pipeline(descriptor)?

    encoder := device.encoder()?
    {
        pass := encoder.compute_pass()
        pass.set_pipeline(&pipeline)
        pass.set_buffer(0, &params)
        pass.set_buffer(1, &x)
        pass.set_buffer(2, &y)
        pass.dispatch(16, 1, 1)
    }
    encoder.submit()?

    result := [0.0f32; 1000]
    device.read_buffer(&y, 0, gpu.f32_bytes_mut(&mut result))?
    // ...
}
```

[`examples/gpu_triangle.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/gpu_triangle.dodo)
draws a spinning triangle and shows it in a [std/window](window.md) window.

## Devices

`gpu.Device.open(gpu.Options.default())` opens the first backend that works,
in the order of the table above. `gpu.Options.with_backend(gpu.Preference.Vulkan)`
asks for one backend (`Vulkan`, `Metal` or `OpenGl`) and fails if it is not
available. Set `debug: true` in the options to turn on the API's own checks:
the Khronos validation layer for Vulkan when it is installed, and a debug
context for OpenGL. For Metal, run with `MTL_DEBUG_LAYER=1` instead.

| Call | Returns |
| --- | --- |
| `device.api()` | `gpu.Api.Vulkan`, `Metal` or `OpenGl`; `gpu.api_name(&api)` names it |
| `device.name()` | The GPU's name, such as `"Apple M2"` |
| `device.limits()` | `gpu.Limits`: the largest texture, buffer, binding and workgroup |
| `device.wait()` | Waits until the GPU has finished everything submitted |

Use a device from the thread that opened it. Everything the device creates
borrows it, so it outlives its buffers, textures and pipelines; keep those
in a struct that borrows the device, or in the same function.

## Buffers

`device.create_buffer(size, usage)` creates zero-filled GPU memory that can be
used in the ways `usage` combines:

| Usage | Allows |
| --- | --- |
| `BUFFER_COPY_SRC` | Copies from it, and `device.read_buffer` |
| `BUFFER_COPY_DST` | Copies into it, and `device.write_buffer` |
| `BUFFER_VERTEX`, `BUFFER_INDEX` | `set_vertex_buffer`, `set_index_buffer` |
| `BUFFER_UNIFORM` | A uniform buffer slot |
| `BUFFER_STORAGE` | A storage buffer slot |

`device.create_buffer_with_data(bytes, usage)` creates a buffer holding a copy
of `bytes`, without needing `BUFFER_COPY_DST`. `gpu.f32_bytes`, `u32_bytes`,
`i32_bytes` and `u16_bytes` view number arrays as bytes, and `f32_bytes_mut`
and friends let `read_buffer` fill them.

`device.write_buffer(&buffer, offset, bytes)` happens after every submission
made before it and before every later one, exactly as if it were a command
of its own: a write never changes what an earlier submission sees.
`device.read_buffer(&buffer, offset, &mut out)` waits for every submission so
far and then copies. Both fail with `InvalidInput` when the range does not
fit or the buffer lacks the usage.

## Textures and samplers

Textures are two-dimensional, with one mipmap level and one layer:

```dodo
texture := device.create_texture(gpu.TextureDescriptor.new(256, 256, gpu.FORMAT_RGBA8_UNORM, gpu.TEXTURE_RENDER | gpu.TEXTURE_COPY_SRC))?
```

| Format | Bytes per texel | Notes |
| --- | --- | --- |
| `FORMAT_R8_UNORM`, `FORMAT_RG8_UNORM` | 1, 2 | |
| `FORMAT_RGBA8_UNORM`, `FORMAT_RGBA8_UNORM_SRGB` | 4 | `FORMAT_RGBA8_UNORM` can be a storage texture |
| `FORMAT_BGRA8_UNORM`, `FORMAT_BGRA8_UNORM_SRGB` | 4 | The layout of a [std/window](window.md) canvas |
| `FORMAT_R16_FLOAT`, `FORMAT_RG16_FLOAT`, `FORMAT_RGBA16_FLOAT` | 2, 4, 8 | `FORMAT_RGBA16_FLOAT` can be a storage texture |
| `FORMAT_R32_FLOAT`, `FORMAT_RG32_FLOAT`, `FORMAT_RGBA32_FLOAT` | 4, 8, 16 | Storage textures; not blendable, and not every GPU filters them linearly |
| `FORMAT_R32_UINT`, `FORMAT_R32_SINT`, `FORMAT_RGBA32_UINT` | 4, 4, 16 | Storage textures; read as integers, so neither filtered nor blended |
| `FORMAT_DEPTH32_FLOAT` | 4 | Render, sample and copy from; not written by copies or stores |

Usages are `TEXTURE_COPY_SRC`, `TEXTURE_COPY_DST`, `TEXTURE_SAMPLED`,
`TEXTURE_STORAGE` and `TEXTURE_RENDER`. Textures start out zero.
`device.write_texture(&texture, region, bytes, bytes_per_row)` and
`device.read_texture(&texture, region, &mut out, bytes_per_row)` copy a
`gpu.Region` of texels, ordered like the buffer calls. Rows are
`bytes_per_row` apart, a multiple of the texel size and at least the width of
the region; `texture.region()` covers the whole texture and
`texture.bytes_per_row()` is its tight row size. Rows run top to bottom:
row 0 is the top row in memory, on screen and at texture coordinate v = 0.

`device.create_sampler(gpu.SamplerDescriptor.linear())` and `nearest()`
filter and clamp; set `address_u` and `address_v` to `ADDRESS_REPEAT` or
`ADDRESS_MIRROR` to tile.

## Shaders and bindings

A `gpu.ShaderSource` holds one shader in the language of every backend, and a
backend uses its own: SPIR-V words for Vulkan, Metal Shading Language source
for Metal, GLSL source for OpenGL. Leave out what you do not target; a device
whose language is missing fails with `MissingShader`. `gpu.ShaderSource.new()`
with `with_spirv`, `with_msl` and `with_glsl` builds one piece by piece.

Shaders find their resources in numbered slots, 0 to 15, with the same number
in every language:

| Binding kind | SPIR-V (GLSL for Vulkan) | Metal | GLSL for OpenGL |
| --- | --- | --- | --- |
| `BindingLayout.uniform_buffer(n, stages)` | `layout(binding = n) uniform` | `constant T &x [[buffer(n)]]` | `layout(std140, binding = n) uniform` |
| `storage_buffer`, `read_only_storage_buffer` | `layout(binding = n) buffer` | `device T *x [[buffer(n)]]` | `layout(std430, binding = n) buffer` |
| `sampled_texture` | `layout(binding = n) uniform sampler2D` | `texture2d<float> [[texture(n)]]` and `sampler [[sampler(n)]]` | `layout(binding = n) uniform sampler2D` |
| `storage_texture(n, stages, format)` | `layout(binding = n, r32ui) uniform uimage2D` | `texture2d<uint, access::write> [[texture(n)]]` | the same as SPIR-V |
| Vertex attribute at `location` | `layout(location = l) in` | `[[attribute(l)]]` with `[[stage_in]]` | `layout(location = l) in` |

Vertex buffer *n* is Metal's `[[buffer(16 + n)]]`, which `[[stage_in]]` reads
for you. A pipeline lists its bindings, and every slot it lists must hold a
matching object with the right usage when it draws or dispatches.

The entry point is the function's name in Metal and in the SPIR-V module.
glslangValidator names it `main` unless told otherwise; to use one name for
both, compile with
`glslangValidator -V -S comp -e saxpy --source-entrypoint main shader.glsl`.
GLSL's entry point is always `main`, so OpenGL ignores the name. OpenGL
compiles a GLSL source once per stage, with `#define VERTEX`, `#define
FRAGMENT` or `#define COMPUTE` inserted after its `#version` line, so one file
can hold every stage of a pipeline:

```glsl
#version 450
#ifdef VERTEX
layout(location = 0) in vec2 position;
void main() { gl_Position = vec4(position, 0.0, 1.0); }
#endif
#ifdef FRAGMENT
layout(location = 0) out vec4 color;
void main() { color = vec4(1.0, 0.5, 0.0, 1.0); }
#endif
```

The same GLSL also compiles for Vulkan, which needs `gl_VertexIndex` where
OpenGL has `gl_VertexID`; glslangValidator defines `VULKAN` to tell them
apart.

## Pipelines

```dodo
descriptor := gpu.ComputePipelineDescriptor.new(&source, "saxpy", 64, 1, 1)
descriptor.bindings = &bindings
pipeline := device.create_compute_pipeline(descriptor)?
```

The workgroup size must equal the shader's own (`local_size` in GLSL); Metal,
whose kernels do not declare one, dispatches with it. A render pipeline names
its vertex and fragment shaders, which may share a source, and its color
format:

```dodo
positions := [gpu.VertexAttribute.new(0, gpu.VERTEX_FLOAT32X2, 0)]
colors := [gpu.VertexAttribute.new(1, gpu.VERTEX_UNORM8X4, 0)]
layouts := [gpu.VertexBufferLayout.new(8, &positions), gpu.VertexBufferLayout.new(4, &colors)]
descriptor := gpu.RenderPipelineDescriptor.new(&vertex, "turn", &fragment, "paint", gpu.FORMAT_BGRA8_UNORM)
descriptor.vertex_buffers = &layouts
descriptor.cull_mode = gpu.CULL_BACK
pipeline := device.create_render_pipeline(descriptor)?
```

The other fields default to triangle lists (`topology`), counterclockwise
front faces (`front_face`), no blending (`blend`: `BLEND_ALPHA`,
`BLEND_PREMULTIPLIED` or `BLEND_ADDITIVE`) and no depth buffer
(`depth_format`, with `depth_compare` and `depth_write`).
`VertexBufferLayout.instanced` steps a buffer per instance. Point lists draw
points as large as the vertex shader's `gl_PointSize` or `[[point_size]]`.

Clip space is the same on every backend: x right and y up from -1 to 1,
depth from 0 to 1, and the top of the image at y = 1. Front faces wind
counterclockwise as seen there.

## Commands

`device.encoder()` starts recording. Passes borrow the encoder until they
end, when dropped or with `pass.end()`:

| Command | Does |
| --- | --- |
| `encoder.compute_pass()` | `set_pipeline`, `set_buffer`, `set_buffer_range`, `set_texture`, `set_storage_texture`, `dispatch(x, y, z)` |
| `encoder.render_pass(color)` | The same bindings, plus `set_vertex_buffer`, `set_index_buffer`, `set_viewport`, `set_scissor`, `draw`, `draw_indexed` |
| `encoder.render_pass_with_depth(color, depth)` | A render pass with a depth buffer |
| `encoder.copy_buffer`, `copy_buffer_to_texture`, `copy_texture_to_buffer` | Copies between objects with the copy usages |
| `encoder.submit()` | Runs everything recorded, after all earlier submissions |

`gpu.ColorAttachment.clear(&texture, r, g, b, a)` clears the attachment first
and `ColorAttachment.keep(&texture)` draws over it; `gpu.DepthAttachment` does
the same for depth. Every command sees the results of the commands before it,
including dispatches within one pass. Buffer offsets bound to a slot are
multiples of `gpu.BINDING_ALIGNMENT` (256).

Dropping an encoder without submitting discards its commands, except on
OpenGL, which has no command buffers: there commands run as they are
recorded, and `submit()` only flushes them.

## Object lifetimes

Dropping a buffer, texture, sampler or pipeline that a submitted command or an
open encoder may still use is safe: `std/gpu` destroys it once the GPU has
finished every such submission. `submit()`, `wait()` and `poll()` destroy what
has become free; call `device.poll()` in a program that drops objects and then
submits nothing for a while. At most 64 objects wait at a time. Dropping more
while encoders are open panics, and otherwise waits for the GPU to catch up.

## What is checked

Creating objects and copying data return errors. Recording mistakes panic,
as an index out of bounds does, and the report names the line of
`std/gpu.dodo` whose message describes the mistake:

- usages: every buffer and texture is used only in the ways it was created for
- ranges: copies, bound ranges, texture regions, row sizes and the vertices
  and indices a draw reads all lie inside their buffers and textures
- pipelines: a draw or dispatch has a pipeline, every slot it lists holds the
  right kind of object, storage textures have the layout's format, and the
  attachments match the pipeline's formats and do not appear as bindings
- limits: workgroup counts and sizes, texture sizes and binding sizes
- SPIR-V: Vulkan modules are well formed and contain the named entry point
  of the right stage

The GPU's side is not checked: what a shader reads or writes, and index
values in an index buffer, are up to the program. Vulkan enables
`robustBufferAccess` where the GPU offers it, so out-of-bounds accesses
there read zero instead of other memory; run with the APIs' validation while
developing.

## Errors

`gpu.Error` and `gpu.error_name(error)`:

| Error | When |
| --- | --- |
| `NoBackend` | No backend could be used: no Metal GPU, Vulkan loader or OpenGL library, or no GPU behind it |
| `Unsupported` | The backend is too old (Vulkan before 1.2 or without dynamic rendering and push descriptors, OpenGL before 4.5) or lacks a format |
| `InvalidInput` | An argument breaks one of the rules above |
| `MissingShader` | The shader source has no code for this device's API |
| `InvalidShader` | The shader does not compile or link, or lacks the entry point |
| `OutOfMemory` | The GPU or driver ran out of memory |
| `DeviceLost` | The GPU stopped responding |
| `System` | Another backend call failed |

## Backends

| | Vulkan | Metal | OpenGL |
| --- | --- | --- | --- |
| Version | 1.2 with dynamic rendering and push descriptors (core in 1.3 and 1.4) | Any Mac GPU | 4.5 core, without a window |
| Buffers | Host-visible memory, device-local where possible | Shared storage | Buffer objects |
| Textures | Optimal tiling, always in the general layout | Private storage, copied through buffers | Immutable storage |
| Synchronization | A full barrier before every pass, copy and dispatch; a timeline semaphore numbers submissions | Metal's own tracking; a shared event numbers submissions | `glMemoryBarrier` after every pass and dispatch |

macOS stops at OpenGL 4.1, so there `Preference.OpenGl` fails with
`Unsupported`; Vulkan needs MoltenVK there, as described on the
[Vulkan](vulkan.md) page.

## Limits

There are no window surfaces yet: frames reach a [std/window](window.md)
window by reading the texture back into its canvas, as the triangle example
does. A render pass has one color attachment; textures have no mipmaps,
layers or multisampling; there are no push constants, indirect draws, stencil
buffers or queries. The raw bindings, [std/gpu/vulkan](vulkan.md),
[std/gpu/metal](metal.md) and [std/gpu/opengl](opengl.md), cover all of that.

## Writing a backend

`std/gpu/hal` is the interface between `std/gpu` and its backends. A backend
is a `Device` struct implementing `hal.Backend`: it creates objects from
already-checked descriptors and returns them as `hal.Handle`s, four words whose
meaning it defines, and it records commands into an encoder handle. `std/gpu`
does the checking, keeps each pass's bindings in a `hal.PassState` that the
backend applies at each draw or dispatch, and tells the backend when to destroy
objects. `std/gpu/hal/native` names the operating system's own backend, Metal
on macOS; on Linux and Windows it never opens.
