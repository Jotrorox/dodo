---
title: "Metal"
description: "Use Apple's Metal GPU API from Dodo on macOS: devices, buffers, shaders, compute dispatches, render passes and CAMetalLayer, bound without any C code."
section: "Standard library"
order: 160.7
---

`std/gpu/metal` binds Apple's Metal API for macOS on Apple silicon and Intel.
It is written entirely in Dodo: each Metal method is an `objc_msgSend` declared
with that method's own signature, so there is no C runtime and nothing to
install. Building for a macOS target links Metal, QuartzCore, Foundation and
libobjc automatically.

The module follows Metal closely, one Dodo method per Metal method, so Apple's
documentation applies directly. It is the bottom layer of the portable
[std/gpu](gpu.md) on macOS, and programs can use it on its own.

## Quickstart

This program adds two arrays on the GPU:

```dodo
package main

import "std/console"
import "std/gpu/metal"

const SOURCE: &str = "#include <metal_stdlib>\nusing namespace metal;\nkernel void add(device const float *a [[buffer(0)]], device const float *b [[buffer(1)]], device float *sum [[buffer(2)]], uint i [[thread_position_in_grid]]) {\n    sum[i] = a[i] + b[i];\n}\n"

fn run() -> void!metal.Error {
    device := metal.Device.system_default()?
    queue := device.new_command_queue()?
    options := metal.CompileOptions.new()?
    library := device.new_library_with_source(SOURCE, &options)?
    function := library.new_function("add")?
    pipeline := device.new_compute_pipeline_state(&function)?

    shared := metal.RESOURCE_STORAGE_MODE_SHARED
    a := device.new_buffer(16, shared)?
    b := device.new_buffer(16, shared)?
    sum := device.new_buffer(16, shared)?
    left := [1.0f32, 2.0, 3.0, 4.0]
    right := [10.0f32, 20.0, 30.0, 40.0]
    a.write_f32(0, &left)?
    b.write_f32(0, &right)?

    commands := queue.command_buffer()?
    encoder := commands.compute_command_encoder()?
    encoder.set_compute_pipeline_state(&pipeline)
    encoder.set_buffer(&a, 0, 0)
    encoder.set_buffer(&b, 0, 1)
    encoder.set_buffer(&sum, 0, 2)
    encoder.dispatch_threadgroups(metal.Size.new(1, 1, 1), metal.Size.new(4, 1, 1))
    encoder.end_encoding()
    commands.commit()
    commands.wait_until_completed()?

    result := [0.0f32; 4]
    sum.read_f32(0, &mut result)?
    console.println(result[3] as i32)!
    return ok()
}

fn main() {
    match run() {
        ok() => {}
        err(error) => {
            console.println(metal.error_name(&error))!
        }
    }
}
```

`dodo run add.dodo` prints `44`. Shaders are Metal Shading Language source
compiled at run time; `new_library_with_data` loads a `.metallib` compiled
ahead of time instead.

## Names

Methods drop Metal's argument labels and use snake case. Constants keep the
enumerator's name in upper case, and structs keep their fields:

| Metal | Dodo |
| --- | --- |
| `MTLCreateSystemDefaultDevice()` | `metal.Device.system_default()` |
| `[device newBufferWithLength:options:]` | `device.new_buffer(length, options)` |
| `[encoder setBuffer:offset:atIndex:]` | `encoder.set_buffer(&buffer, offset, index)` |
| `[descriptor colorAttachments][0]` | `descriptor.color_attachment(0)?` |
| `MTLPixelFormatBGRA8Unorm_sRGB` | `metal.PIXEL_FORMAT_BGRA8_UNORM_SRGB` |
| `MTLResourceStorageModePrivate` | `metal.RESOURCE_STORAGE_MODE_PRIVATE` |
| `MTLSizeMake(64, 1, 1)` | `metal.Size.new(64, 1, 1)` |
| `MTLRegionMake2D(x, y, w, h)` | `metal.Region.new_2d(x, y, w, h)` |

Option sets combine with `|`, as in Metal:
`metal.TEXTURE_USAGE_SHADER_READ | metal.TEXTURE_USAGE_RENDER_TARGET`. The test
suite checks every constant against the installed SDK's headers.

## Objects

Every Metal object is an owned strong reference. Dropping it releases the
reference, and `clone()` retains another one. Moving a value moves the
reference, so a program cannot release an object twice or use one it has
released. Encoders take `end_encoding(self)` by value, so an ended encoder
cannot be used again.

Metal keeps objects alive while the GPU needs them: a command buffer retains
the buffers, textures and pipelines its commands use until it completes. Only
`command_buffer_with_unretained_references` leaves that to the program.

Devices, command queues, buffers, textures, libraries, functions, pipeline
states, samplers, depth-stencil states, fences and events may be sent to and
shared between [threads](threads.md); Metal documents them as thread-safe.
A command buffer may be sent to another thread but not shared. Encoders and
descriptors stay on the thread that made them.

### Autorelease pools

Cocoa code expects to run inside an autorelease pool, and frameworks
autorelease objects internally. The bindings wrap every call that creates,
returns or destroys objects in a pool of its own, so a command-line program
does not accumulate them. For a long-running loop, also wrap each iteration in
a pool, which catches what Metal autoreleases elsewhere:

```dodo
for {
    pool := objc.AutoreleasePool.new()
    draw_frame(&queue, &layer)?
    core.drop(pool)
}
```

`objc` is `std/gpu/metal/objc`.

## Errors

Calls that can fail return `T!metal.Error`:

| Variant | Meaning |
| --- | --- |
| `NoDevice` | The system has no Metal GPU, as in most virtual machines. |
| `OutOfMemory` | Metal returned no object. |
| `InvalidInput` | An argument is outside what the call accepts, such as a byte range past the end of a buffer. |
| `NotFound` | A library has no function with the requested name. |
| `Unsupported` | The call needs something unavailable, such as a pixel format whose size is unknown. |
| `Failed(objc.NSError)` | Metal reported an error. |

`Failed` carries Metal's own `NSError`. For a shader that does not compile,
its message is the compiler's log:

```dodo
match device.new_library_with_source(source, &options) {
    ok(library) => {
        compiled(library)
    }
    err(metal.Error.Failed(error)) => {
        if let some(message) = error.message() {
            log := [0u8; 4096]
            length := message.copy_to(&mut log)
            // log[..length] is valid UTF-8, cut at a character boundary.
        }
    }
    err(other) => {
        report(other)
    }
}
```

`commands.wait_until_completed()` returns `Failed` the same way when the GPU
could not run a command buffer.

## What is checked

The checks cover everything that would let Metal read or write CPU memory
outside a Dodo slice or resource:

- `Buffer.write`, `read`, `write_f32`, `read_f32`, `write_u32` and `read_u32`
  check the byte range against the buffer, and reject private buffers.
- `Texture.write_pixels` and `read_pixels` check the region, mipmap level,
  slice, row stride and slice length against the texture and its pixel
  format. Compressed and combined depth-stencil formats are `Unsupported`
  there; the unsafe `replace_region` and `get_bytes` copy them unchecked.
- `set_bytes` and friends pass the slice's own length.
- Binding indices past `MAX_BUFFER_BINDINGS` (31), `MAX_TEXTURE_BINDINGS`
  (128) and `MAX_SAMPLER_BINDINGS` (16) panic, as an out-of-bounds index does.

The GPU's side is not checked: shaders, draws, dispatches and blits run
exactly as encoded, as they do in Metal itself. Run with Metal's validation
layer while developing to catch mistakes there:

```sh
MTL_DEBUG_LAYER=1 MTL_SHADER_VALIDATION=1 ./program
```

`Buffer.contents()` returns the raw CPU address of a shared buffer for code
that maps its own structures; dereferencing it needs `unsafe`.

## Rendering

A render pass draws into textures described by a `RenderPassDescriptor`.
This one clears a 256 x 256 texture and draws one triangle with a pipeline
built from `RenderPipelineDescriptor`:

```dodo
usage := metal.TEXTURE_USAGE_RENDER_TARGET | metal.TEXTURE_USAGE_SHADER_READ
descriptor := metal.TextureDescriptor.new_2d(metal.PIXEL_FORMAT_RGBA8_UNORM, 256, 256, usage)?
target := device.new_texture(&descriptor)?

pass := metal.RenderPassDescriptor.new()?
color := pass.color_attachment(0)?
color.set_texture(&target)
color.set_load_action(metal.LOAD_ACTION_CLEAR)
color.set_store_action(metal.STORE_ACTION_STORE)
color.set_clear_color(metal.ClearColor.new(0.1, 0.1, 0.1, 1.0))

commands := queue.command_buffer()?
encoder := commands.render_command_encoder(&pass)?
encoder.set_render_pipeline_state(&pipeline)
encoder.set_vertex_buffer(&vertices, 0, 0)
encoder.draw_primitives(metal.PRIMITIVE_TYPE_TRIANGLE, 0, 3)
encoder.end_encoding()
commands.commit()
```

On Apple silicon, textures can use shared storage and `read_pixels` reads
them directly after the GPU finishes. Intel Macs with a discrete GPU need
`STORAGE_MODE_MANAGED` and a `synchronize_texture` blit first.

### Showing frames

`MetalLayer` is a `CAMetalLayer`. Give it a device, a pixel format and a size,
then take one drawable per frame:

```dodo
layer := metal.MetalLayer.new()?
layer.set_device(&device)
layer.set_pixel_format(metal.PIXEL_FORMAT_BGRA8_UNORM)
layer.set_drawable_size(1600.0, 1200.0)

if let some(drawable) = layer.next_drawable() {
    frame := drawable.texture()?
    // ... render into frame ...
    commands.present_drawable(&drawable)
    commands.commit()
}
```

`attach_to_view` makes the layer the backing layer of an AppKit `NSView`.

### Presenting to a window

A [std/window](window.md#drawing-with-the-gpu) window opened with
`window.Options.gpu()` gives its view a `CAMetalLayer` through
`win.metal_layer()`. The view keeps the layer; wrap a reference of your own
and render into its drawables:

```dodo
app := window.App.new()?
win := app.open(window.Options.gpu("Metal", 480, 320))?
address := win.metal_layer()?
// A second reference to the layer, released when `layer` is dropped.
object := match unsafe {
    objc.Object.retain(address as objc.Id)
}
{
    some(object) => object,
    none => {
        return err(Failure.NoLayer)
    }
}
layer := unsafe {
    metal.MetalLayer.from_object(object)
}
layer.set_device(&device)
layer.set_pixel_format(metal.PIXEL_FORMAT_BGRA8_UNORM)
```

`std/window` keeps the layer's drawable size at the window's size in pixels
and its contents scale at the display's, so a frame needs no sizing of its
own: take `next_drawable()` on each `Event.Frame` that `win.request_frame()`
asked for, render into its texture and `present_drawable` it.
[`examples/metal_window.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/metal_window.dodo)
draws a spinning triangle this way.

## Extending the bindings

`std/gpu/metal/objc` is the Objective-C runtime layer the bindings use, and it
is public so programs can call methods the module does not cover. Declare one
`objc_msgSend` per method signature with `@link_name`, look selectors up once
with `cached_selector`, and pass objects with `as_object().as_ptr()`:

```dodo
import "core/ptr"
import "std/gpu/metal"
import "std/gpu/metal/objc"

@link_name("objc_msgSend")
unsafe extern "C" fn send_usize(receiver: *mut u8, selector: *mut u8) -> usize

static mut MAX_ARGUMENT_BUFFER_SAMPLERS: usize = 0

fn max_argument_buffer_samplers(device: &metal.Device) -> usize {
    unsafe {
        selector := objc.cached_selector(ptr.from_mut(&mut MAX_ARGUMENT_BUFFER_SAMPLERS), b"maxArgumentBufferSamplerCount\0")
        return send_usize(device.as_object().as_ptr(), selector)
    }
}
```

`objc.Object.from_retained` adopts a returned object the caller owns (from a
method named `new…`, `alloc`, `copy…` or `mutableCopy…`), and
`objc.Object.retain` retains any other. `metal.Device.from_object` and the
other `from_object` functions wrap such an object in its typed handle.
`objc.NSString`, `objc.NSError` and `objc.NSArray` cover the Foundation types
Metal uses.
