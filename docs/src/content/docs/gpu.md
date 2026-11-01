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
freed memory or copy outside a buffer. Shaders can be written once in WGSL,
frames go to [std/window](window.md) windows through `gpu.Surface`, and the
CPU runs ahead of the GPU: writing data never waits for it.

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
the program, with the WGSL kernel (see [Shaders in WGSL](#shaders-in-wgsl))
left out:

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

    // The kernel, translated from WGSL for every backend.
    source := kernel()
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

[`examples/gpu_window.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/gpu_window.dodo)
draws a spinning triangle into a [std/window](window.md) window through a
`gpu.Surface` (see [Surfaces](#surfaces)), and
[`examples/gpu_triangle.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/gpu_triangle.dodo)
does the same by reading each frame back into the window's canvas.

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
| `device.limits()` | `gpu.Limits`: the largest texture, buffer, binding and workgroup, and how many color attachments a pass takes (at least 4) |
| `device.wait()` | Waits until the GPU has finished everything submitted, and every write |
| `device.poll()` | Starts pending writes and destroys dropped objects the GPU is done with |

`gpu.Device.open_for_window(&native, options)` opens a device that can
present to a window, for a [Surface](#surfaces): `native` comes from
std/window's `win.native_handle()`. On OpenGL the device then draws with the
window's own context; Vulkan picks a GPU that can present to the window.

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
of its own: a write never changes what an earlier submission sees. It does
not wait for the GPU: the bytes go into a staging buffer, and the copy runs
just before the next submission (see [Staging and frames in
flight](#staging-and-frames-in-flight)). `device.read_buffer(&buffer, offset,
&mut out)` waits for every submission so far and then copies. Both fail with
`InvalidInput` when the range does not fit or the buffer lacks the usage.

## Textures and samplers

Textures are two-dimensional, with one layer and one or more mipmap levels:

```dodo
texture := device.create_texture(gpu.TextureDescriptor.new(256, 256, gpu.FORMAT_RGBA8_UNORM, gpu.TEXTURE_RENDER | gpu.TEXTURE_COPY_SRC))?
image := device.create_texture(gpu.TextureDescriptor.mipmapped(256, 256, gpu.FORMAT_RGBA8_UNORM, gpu.TEXTURE_SAMPLED | gpu.TEXTURE_COPY_DST))?
samples := device.create_texture(gpu.TextureDescriptor.multisampled(256, 256, gpu.FORMAT_RGBA8_UNORM))?
```

`TextureDescriptor.mipmapped` gives a texture every level down to one texel
(`gpu.mip_level_count(width, height)` of them); set `mip_levels` for fewer.
Each level is half the size of the one before, rounded down to at least 1
(`gpu.level_size`). Write the first level and fill the others with
`encoder.generate_mipmaps(&texture)`, which filters each level down from the
one before for 8-bit and 16-bit float color formats, or write each level
yourself. Render passes draw into the first level, and storage bindings
reach only that level too.

`TextureDescriptor.multisampled` makes a render target with
`gpu.MULTISAMPLE_COUNT` (4) samples per pixel, which a render pass resolves
into an ordinary texture of the same size and format (see
[Commands](#commands)). Multisampled textures have one level, no integer
format and only the usage `TEXTURE_RENDER`: nothing copies, samples or
stores them.

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
`gpu.Region` of texels, ordered and staged like the buffer calls. Rows are
`bytes_per_row` apart, a multiple of the texel size and at least the width of
the region; `texture.region()` covers the whole first level and
`texture.bytes_per_row()` is its tight row size, while
`texture.level_region(level)` and `texture.level_bytes_per_row(level)` do the
same for any level, and `gpu.Region.at_level(level, x, y, width, height)`
picks part of one. Rows run top to bottom: row 0 is the top row in memory,
on screen and at texture coordinate v = 0.

`device.create_sampler(gpu.SamplerDescriptor.linear())` and `nearest()`
filter and clamp, and `trilinear()` filters between texels and mipmap levels
and repeats. A `gpu.SamplerDescriptor` has these fields:

| Field | Sets |
| --- | --- |
| `mag_filter`, `min_filter` | `FILTER_NEAREST` or `FILTER_LINEAR` between texels |
| `mipmap_filter` | `FILTER_NEAREST` takes the nearest level, `FILTER_LINEAR` blends the two nearest |
| `address_u`, `address_v` | `ADDRESS_CLAMP`, `ADDRESS_REPEAT`, `ADDRESS_MIRROR`, or `ADDRESS_BORDER`, which reads `border` outside the texture |
| `border` | `BORDER_TRANSPARENT_BLACK`, `BORDER_OPAQUE_BLACK` or `BORDER_OPAQUE_WHITE` |
| `lod_min`, `lod_max` | The range of levels sampled, 0 to 32 by default |
| `max_anisotropy` | Up to 16 samples along the direction a texture is stretched in; a hint, and above 1 it needs every filter linear |
| `compare` | `COMPARE_NONE`, or a `COMPARE_*` that makes a comparison sampler |

A comparison sampler compares a reference depth with the texels of a depth
texture, giving 1.0 where the comparison passes and 0.0 where it fails,
filtered when the filters are linear. `SamplerDescriptor.comparison(gpu.COMPARE_LESS_EQUAL)`
is the usual one for shadow maps: linear, and passing outside the map, where
it reads an opaque white border. Comparison samplers only work in
`comparison_texture` slots, which only take them.

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
| `comparison_texture` | `layout(binding = n) uniform sampler2DShadow` | `depth2d<float> [[texture(n)]]` and `sampler [[sampler(n)]]` | `layout(binding = n) uniform sampler2DShadow` |
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

## Shaders in WGSL

`gpu.wgsl` takes a shader written once in [WGSL](https://www.w3.org/TR/WGSL/)
and translates it when the program is compiled, into SPIR-V, Metal Shading
Language and GLSL 4.50, so one source runs on every backend with no tools to
install:

```dodo
fn kernels() -> gpu.ShaderSource from(static) {
    return gpu.wgsl(
        "@group(0) @binding(0) var<uniform> a: f32;",
        "@group(0) @binding(1) var<storage, read> x: array<f32>;",
        "@group(0) @binding(2) var<storage, read_write> y: array<f32>;",
        "",
        "@compute @workgroup_size(64)",
        "fn saxpy(@builtin(global_invocation_id) id: vec3<u32>) {",
        "    y[id.x] = a * x[id.x] + y[id.x];",
        "}"
    )
}
```

The arguments are string literals, one or more, joined with line breaks; a
mistake in the shader is a compile error that points into the literal. The
result is an ordinary `ShaderSource` whose SPIR-V lives in static storage,
so it can be bound to a local (`source := gpu.wgsl(...)`) or returned from a
function declared `from(static)` as above, and passed to pipeline
descriptors like any other. A module may hold many entry points; pass the
WGSL function's name as the entry point, the same on every backend.

Bindings use std/gpu's slots:

| WGSL | std/gpu |
| --- | --- |
| `@group(0) @binding(n) var<uniform>` | `BindingLayout.uniform_buffer(n, stages)` |
| `@group(0) @binding(n) var<storage, read_write>` or `var<storage, read>` | `storage_buffer` or `read_only_storage_buffer` |
| `@group(0) @binding(n) var t: texture_2d<f32>` and `@group(1) @binding(n) var s: sampler` | `sampled_texture(n, stages)`, bound with `set_texture(n, texture, sampler)` |
| `@group(0) @binding(n) var t: texture_depth_2d` and `@group(1) @binding(n) var s: sampler_comparison` | `comparison_texture(n, stages)`, bound with a depth texture and a comparison sampler |
| `@group(0) @binding(n) var t: texture_storage_2d<r32uint, write>` | `storage_texture(n, stages, gpu.FORMAT_R32_UINT)` |
| `@location(l)` input of a `@vertex` function | `VertexAttribute.new(l, format, offset)` |
| `@location(n)` output of a `@fragment` function | Color target *n* of the pipeline |

So samplers sit in `@group(1)`, at the binding of the texture they are read
with in `@group(0)`; every other resource is in `@group(0)`, at slots 0 to
15. Different entry points of one module may use the same slot for
different resources. Clip space and depth are as WGSL defines them, which is
what std/gpu uses on every backend. The translation is done by
[naga](https://github.com/gfx-rs/wgpu/tree/trunk/naga), and refuses what
some backend cannot run as std/gpu drives it:

- `arrayLength` (Metal would need buffer sizes std/gpu does not pass): put
  the length in a uniform buffer.
- Immediate data (push constants): use a uniform buffer.
- Entry point names Metal reserves, such as `main`, and names that are not
  ASCII identifiers.

A compiler built without its default `wgsl` feature reports `gpu.wgsl` calls
as errors; `ShaderSource` with hand-written languages keeps working.

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
`BLEND_PREMULTIPLIED` or `BLEND_ADDITIVE`), no depth buffer (`depth_format`,
with `depth_compare` and `depth_write`), no depth bias and one sample per
pixel. `VertexBufferLayout.instanced` steps a buffer per instance. Point
lists draw points as large as the vertex shader's `gl_PointSize` or
`[[point_size]]`.

For several color attachments, or blending beyond the presets, pass
`gpu.FORMAT_NONE` as the color format and list `targets` instead; target
*n* takes the fragment shader's output at location *n*:

```dodo
targets := [
    gpu.ColorTarget.new(gpu.FORMAT_RGBA16_FLOAT),
    gpu.ColorTarget.blended(gpu.FORMAT_RGBA8_UNORM, gpu.Blend.new(gpu.BLEND_FACTOR_CONSTANT, gpu.BLEND_FACTOR_ONE_MINUS_CONSTANT, gpu.BLEND_OP_ADD)),
]
targets[1].write_mask = gpu.COLOR_WRITE_RED | gpu.COLOR_WRITE_GREEN | gpu.COLOR_WRITE_BLUE
descriptor := gpu.RenderPipelineDescriptor.new(&source, "scene_vertex", &source, "scene_fragment", gpu.FORMAT_NONE)
descriptor.targets = &targets
```

A `gpu.Blend` computes `source * source factor (op) destination * destination factor`,
separately for color and alpha with `Blend.separate`. The factors are
`BLEND_FACTOR_ZERO`, `ONE`, `SRC`, `ONE_MINUS_SRC`, `SRC_ALPHA`,
`ONE_MINUS_SRC_ALPHA`, `DST`, `ONE_MINUS_DST`, `DST_ALPHA`,
`ONE_MINUS_DST_ALPHA`, `SRC_ALPHA_SATURATED`, `CONSTANT` and
`ONE_MINUS_CONSTANT` (the pass's `set_blend_constant`), and the operations
`BLEND_OP_ADD`, `SUBTRACT`, `REVERSE_SUBTRACT`, `MIN` and `MAX`;
`Blend.preset(gpu.BLEND_ALPHA)` spells out a preset. A pass may have up to
`limits().max_color_attachments` targets. Vulkan GPUs without the
`independentBlend` feature, which are rare, need every target blended and
masked alike.

A pipeline that only writes depth, such as one drawing a shadow map, leaves
out the fragment stage and draws in a `depth_pass`:

```dodo
descriptor := gpu.RenderPipelineDescriptor.depth_only(&source, "shadow_vertex", gpu.FORMAT_DEPTH32_FLOAT)
descriptor.depth_bias = 2
descriptor.depth_bias_slope_scale = 1.5
```

`depth_bias` adds that many of the depth buffer's smallest steps to each
fragment's depth, `depth_bias_slope_scale` scales the triangle's depth
slope, and `depth_bias_clamp`, unless 0, limits the sum; it applies to
triangles only. Later passes read the map through a `comparison_texture`
slot. For multisampling, set `samples` to `gpu.MULTISAMPLE_COUNT` on a
pipeline drawing into multisampled attachments; `alpha_to_coverage` then
covers each pixel's samples in proportion to the alpha written to location
0, for antialiased cutouts such as foliage.

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
| `encoder.render_pass_targets(&colors)`, `render_pass_targets_with_depth(&colors, depth)` | A render pass with several color attachments, one per pipeline target |
| `encoder.depth_pass(depth)` | A render pass with only a depth buffer, for depth-only pipelines |
| `pass.set_blend_constant(r, g, b, a)` | The color `BLEND_FACTOR_CONSTANT` reads, until the pass ends (0 to begin with) |
| `encoder.generate_mipmaps(&texture)` | Fills every level after the first from the one before |
| `encoder.copy_buffer`, `copy_buffer_to_texture`, `copy_texture_to_buffer` | Copies between objects with the copy usages |
| `encoder.submit()` | Runs everything recorded, after all earlier submissions |

`gpu.ColorAttachment.clear(&texture, r, g, b, a)` clears the attachment first
and `ColorAttachment.keep(&texture)` draws over it; `gpu.DepthAttachment` does
the same for depth. All attachments of a pass have one size and one sample
count. A multisampled color attachment resolves into a texture with one
sample when the pass ends, and `discard()` drops what the GPU need not keep
afterwards, leaving it undefined:

```dodo
color := gpu.ColorAttachment.clear(&samples, 0.0, 0.0, 0.0, 1.0).with_resolve(frame.texture()).discard()
pass := encoder.render_pass_with_depth(color, gpu.DepthAttachment.clear(&depth, 1.0).discard())
```

Every command sees the results of the commands before it, including
dispatches within one pass. Buffer offsets bound to a slot are multiples of
`gpu.BINDING_ALIGNMENT` (256).

Dropping an encoder without submitting discards its commands, except on
OpenGL, which has no command buffers: there commands run as they are
recorded, and `submit()` only flushes them. For the same reason, a write
made on OpenGL while an encoder is open lands after the commands that
encoder recorded so far, not before all of them.

## Surfaces

A `gpu.Surface` shows frames in a [std/window](window.md) window opened
with `window.Options.gpu`, which has no pixel buffer of its own. Each frame,
`acquire()` an image, render into `frame.texture()` and `present()` it:

```dodo
native := win.native_handle()!
device := gpu.Device.open_for_window(&native, gpu.Options.default())?
surface := gpu.Surface.create(&device, &native, win.width(), win.height())?
// Pipelines drawing into it use surface.format().
// ... for every window.Event.Frame:
frame := surface.acquire()?
encoder := device.encoder()?
{
    pass := encoder.render_pass(gpu.ColorAttachment.clear(frame.texture(), 0.0, 0.0, 0.0, 1.0))
    // set_pipeline, draw, ...
}
encoder.submit()?
frame.present()?
// ... and for every window.Event.Resize:
surface.resize(win.width(), win.height())?
```

The images have an 8-bit sRGB format, `FORMAT_BGRA8_UNORM_SRGB` or
`FORMAT_RGBA8_UNORM_SRGB` (`surface.format()`), the size `surface.width()`
by `surface.height()`, and the usages `TEXTURE_RENDER`, `TEXTURE_COPY_SRC`
and `TEXTURE_COPY_DST`, so a program drawing on the CPU can also
`write_texture` its picture straight into the frame. Presenting waits for
every submission so far, and for writes made before it.

`gpu.Surface.create_with` takes `gpu.SurfaceOptions`: `vsync` (on by
default) ties presenting to the display's refresh, and `frames_in_flight`
(2 by default, or 3) is how many frames the CPU may record before the GPU
has finished the oldest; `acquire()` waits for that. `set_vsync` changes it
later. A frame dropped without `present()` is presented anyway.

`acquire()` fails with `Outdated` when the window's size changed in a way
the surface could not follow by itself, or while it has no size, as when the
window is minimized (`resize(0, 0)`): skip the frame and resize the surface
to the window's size. Vulkan follows size changes by itself on Linux and
Windows; Metal and OpenGL need `resize`.

| Backend | Presents through |
| --- | --- |
| Vulkan | A swapchain on `VK_KHR_xcb_surface` (Linux) or `VK_KHR_win32_surface` (Windows), FIFO with vsync and mailbox or immediate without |
| Metal | The view's `CAMetalLayer`: the one `win.metal_layer()` made, or a new one |
| OpenGL | A texture that `present()` copies into the window's framebuffer with the window's context, then swaps; the device must come from `open_for_window` for that window |

Vulkan surfaces on macOS are not supported; there `Preference.Auto` picks
Metal. The window must stay open while its surface exists, and both are used
from the thread that opened the window.

## Staging and frames in flight

`std/gpu` keeps the CPU from waiting for the GPU. On Vulkan and Metal,
`write_buffer`, `write_texture` and `create_buffer_with_data` copy the data
into a staging ring buffer in memory the GPU can read, and record the copy
into an upload command buffer that is submitted just ahead of the next
`submit()`, `present()`, read or `wait()`. Nothing waits until the ring is
full of copies the GPU has not run yet: it starts at 4 MiB and doubles up to
64 MiB, and writes of more than 16 MiB get a staging buffer of their own.
Several writes to the same buffer before a submission land in the order they
were made. OpenGL's driver stages such calls itself, so there they go
straight to it.

Reading back is the only call that waits: `read_buffer` and `read_texture`
wait for every submission so far, then copy. On Vulkan, buffers live in GPU
memory; where the CPU can map that memory (GPUs with unified memory, or
resizable BAR) reads copy from it directly, and otherwise through a readback
buffer in memory the CPU caches. With a [Surface](#surfaces), two or three
frames are in flight.

Setting the environment variable `DODO_GPU_UNMAPPED` makes the Vulkan
backend treat every buffer as unmapped, as on a GPU with memory of its own,
so the copy paths can be tested on any machine.

## Synchronization

Commands see the results of the ones before them without any calls from the
program. `std/gpu` tracks how every command uses each buffer and texture
(vertex input, uniform or storage reads, storage writes, attachments,
copies) and asks the backend for a barrier only where a command depends on
an earlier one of the same encoder, with exactly the stages and accesses
involved: a compute pass writing a buffer that a render pass then reads as
vertices gets one barrier from compute writes to vertex input, and
independent dispatches, copies and passes get none. Each submission starts
after the ones before it have finished.

The Vulkan backend records each render pass into a secondary command
buffer, so that the barriers its draws need can run before the pass;
OpenGL needs `glMemoryBarrier` only after shader writes to storage buffers
and textures, and Metal tracks hazards itself. Draws of one render pass are
not ordered against each other: a draw cannot read what an earlier draw of
the same pass wrote to a storage buffer or texture.

## Object lifetimes

Dropping a buffer, texture, sampler or pipeline that a submitted command or an
open encoder may still use is safe: `std/gpu` destroys it once the GPU has
finished every such submission. `submit()`, `wait()` and `poll()` destroy what
has become free; call `device.poll()` in a program that drops objects and then
submits nothing for a while. At most 64 objects wait at a time; dropping more
waits for the GPU to finish what was submitted, and panics only when all of
them may still be used by encoders that are open.

## What is checked

Creating objects and copying data return errors. Recording mistakes panic,
as an index out of bounds does, and the report names the line of
`std/gpu.dodo` whose message describes the mistake:

- usages: every buffer and texture is used only in the ways it was created for
- ranges: copies, bound ranges, texture regions, row sizes and the vertices
  and indices a draw reads all lie inside their buffers and textures
- pipelines: a draw or dispatch has a pipeline, every slot it lists holds the
  right kind of object (comparison samplers and depth textures only in
  comparison slots), storage textures have the layout's format, and the
  attachments match the pipeline's formats and sample count and do not
  appear as bindings
- passes: attachments share a size and sample count, no texture is attached
  twice, and resolve targets have one sample and their attachment's format
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
| `Unsupported` | The backend is too old (Vulkan before 1.2 or without dynamic rendering and push descriptors, OpenGL before 4.5), lacks a format or sample count, or lacks an optional feature a pipeline uses (independent blending or depth bias clamping on Vulkan, depth bias clamping before OpenGL 4.6) |
| `InvalidInput` | An argument breaks one of the rules above |
| `MissingShader` | The shader source has no code for this device's API |
| `InvalidShader` | The shader does not compile or link, or lacks the entry point |
| `OutOfMemory` | The GPU or driver ran out of memory |
| `DeviceLost` | The GPU stopped responding |
| `Outdated` | A surface no longer matches its window; resize it |
| `System` | Another backend call failed |

## Backends

| | Vulkan | Metal | OpenGL |
| --- | --- | --- | --- |
| Version | 1.2 with dynamic rendering and push descriptors (core in 1.3 and 1.4) | Any Mac GPU | 4.5 core, without a window |
| Memory | Large blocks per memory type, carved into buffers and images | Metal's own | The driver's |
| Buffers | Device-local, mapped too on unified memory; staged writes, reads through host-cached memory | Shared storage; staged writes | Buffer objects |
| Textures | Optimal tiling, always in the general layout; mipmaps by blits, resolves at the end of rendering | Private storage, copied through buffers | Immutable storage; resolves by framebuffer blits |
| Synchronization | Barriers from tracked usage, one per submission besides; a timeline semaphore numbers submissions | Metal's own tracking; a shared event numbers submissions | `glMemoryBarrier` after shader storage writes |
| Surfaces | Swapchains (Linux, Windows) | `CAMetalLayer` | The window's context |

macOS stops at OpenGL 4.1, so there `Preference.OpenGl` fails with
`Unsupported`; Vulkan needs MoltenVK there, as described on the
[Vulkan](vulkan.md) page.

## Limits

Textures have one layer (no arrays, cube maps or 3D textures), render passes
draw into the first mipmap level, multisampling has 4 samples, and depth
attachments do not resolve; there are no push constants, indirect draws,
stencil buffers or queries. The raw bindings, [std/gpu/vulkan](vulkan.md),
[std/gpu/metal](metal.md) and [std/gpu/opengl](opengl.md), cover all of that.

## Writing a backend

`std/gpu/hal` is the interface between `std/gpu` and its backends. A backend
is a `Device` struct implementing `hal.Backend`: it creates objects from
already-checked descriptors and returns them as `hal.Handle`s, four words whose
meaning it defines, and it records commands into an encoder handle. `std/gpu`
does the checking, keeps each pass's bindings in a `hal.PassState` that the
backend applies at each draw or dispatch, and tells the backend when to destroy
objects. A backend whose `staged()` is true leaves transfers to `std/gpu`,
which copies through `BUFFER_UPLOAD` and `BUFFER_READBACK` buffers it maps
with `mapped()`; `barrier(encoder, before, after)` receives the `USE_*`
bits of the dependent uses; and `create_surface`, `configure_surface`,
`acquire` and `present` drive surfaces. `std/gpu/hal/native` names the
operating system's own backend, Metal on macOS; on Linux and Windows it
never opens.
