---
title: "OpenGL"
description: "Create an OpenGL context without a window on Linux, macOS or Windows, call the complete OpenGL 4.6 core API, and read rendered pixels back."
section: "Standard library"
order: 160.7
---

`std/gpu/opengl` creates OpenGL contexts and offscreen render targets.
`std/gpu/opengl/gl` is the raw OpenGL binding: every enum and command of the
OpenGL 4.6 core profile, generated from the Khronos registry. Both are written
in Dodo. Nothing is linked at build time: each operating system's own OpenGL
interface is opened when a context is created. The portable
[std/gpu](gpu.md) uses OpenGL 4.5 where Vulkan is missing.

| System | Interface | Loaded at run time | Highest version |
| --- | --- | --- | --- |
| Linux (x86-64, AArch64) | EGL, with no X server or Wayland needed | `libEGL.so.1` (Mesa or a vendor driver) | What the driver offers: 4.6 on current Mesa and NVIDIA, 4.5 on llvmpipe |
| macOS (Apple silicon, Intel) | CGL | `OpenGL.framework` | 4.1 core |
| Windows (x64) | WGL, with a hidden window | `opengl32.dll`, `user32.dll`, `gdi32.dll` | What the GPU driver offers |

Because nothing is linked, a program that uses OpenGL still starts on a
machine without it. `Context.create` then returns an error, and the program
can fall back to drawing with [std/graphics](graphics.md).

## Quickstart

This program clears a 64×48 target to orange, reads the pixels back and
checks one:

```dodo
package main

import "std/console"
import "std/gpu/opengl"
import "std/gpu/opengl/gl"

fn main() {
    context := opengl.Context.create(opengl.Options.default())!
    console.println(context.renderer())!
    target := opengl.Target.new(64, 48)!
    unsafe {
        gl.clear_color(1.0, 0.5, 0.0, 1.0)
        gl.clear(gl.COLOR_BUFFER_BIT)
    }
    pixels := [0u8; 12288]
    target.read_rgba(&mut pixels)!
    core.assert_eq(pixels[0], 255u8)
}
```

```sh
dodo run orange.dodo
```

It prints the renderer, such as `Apple M2` or `llvmpipe (LLVM 19.1.7, 128 bits)`.
[`examples/opengl_window.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/opengl_window.dodo)
draws a shaded triangle and shows it in a [std/window](window.md) window.

## Contexts

`opengl.Context.create(options)` creates a context, makes it current on the
calling thread and loads the `gl` package's entry points.
`opengl.Options.new(major, minor)` asks for a version, and
`opengl.Options.default()` asks for 3.3, which every desktop driver from the
last decade offers, and so does macOS. Versions 3.2 and later use the core
profile, the one the `gl` package binds. Earlier versions get a legacy
context. Set `debug: true` in the options for a debug context, which reports
errors through `gl.debug_message_callback`.

The driver may provide a newer compatible version than the one requested:
`context.major()` and `context.minor()` report it. `renderer()`, `version()`
and `vendor()` return the driver's own descriptions.

OpenGL state belongs to the thread a context is current on. With several
contexts, call `context.make_current()` before using one: the `gl` commands
go to the context that was made current last. Dropping a context destroys it.
If it was current, the `gl` entry points are cleared, so later calls panic
instead of reaching a destroyed context.

## Drawing offscreen

A context has no window, so drawing goes to a framebuffer object.
`opengl.Target.new(width, height)` creates one with 8-bit RGBA color and a
24-bit depth and 8-bit stencil buffer, binds it and sets the viewport to cover
it. `target.bind()` makes it the drawing target again after you bind another.
Targets need OpenGL 3.0. They are deleted when dropped, which has to happen
while their context is current.

`target.read_rgba(&mut pixels)` and `target.read_bgra(&mut pixels)` wait for
drawing to finish and copy the pixels out, four bytes each, rows top to bottom
as [std/graphics](graphics.md) stores them. `bytes_needed()` gives the size,
`width * height * 4`. BGRA is `graphics.PixelFormat.Bgra8888`, the layout of a
window canvas, so a frame reaches the screen as a blit:

```dodo
target.read_bgra(&mut frame)?
image := graphics.Canvas.new(&mut frame, width, height, graphics.PixelFormat.Bgra8888)!
canvas := win.canvas()
canvas.blit(&image, 0, 0)
win.update(canvas.take_dirty())?
```

This copies each frame through the CPU. That is fine for tools, tests and
modest frame rates, but not a substitute for presenting directly from the GPU,
which the package does not do yet.

## The raw binding

`std/gpu/opengl/gl` maps the C API one to one, so the OpenGL reference pages
and tutorials apply directly:

| C | Dodo |
| --- | --- |
| `glClearColor`, `glTexImage2D`, `glUniformMatrix4fv` | `gl.clear_color`, `gl.tex_image_2d`, `gl.uniform_matrix_4fv` |
| `GL_COLOR_BUFFER_BIT` | `gl.COLOR_BUFFER_BIT` |
| `GLenum`, `GLbitfield`, `GLuint` / `GLint`, `GLsizei` | `u32` / `i32` |
| `GLboolean`, `GLchar` | `u8` |
| `GLintptr`, `GLsizeiptr` | `isize` |
| `GLsync`, `void *` | `*mut u8` |
| `const GLchar *const *` | `*const *const u8` |
| `GLDEBUGPROC` | `gl.DebugProc`, an `extern "C" fn` |

Constants are `u32`, or `u64` where the registry says so. Cast them where a
command takes a `GLint`, as in
`gl.tex_parameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR as i32)`.
Strings passed to OpenGL, such as shader sources, need a trailing NUL. Write
them as byte strings ending in `\0`.

Every command is an `unsafe fn`. Like their C counterparts, they take raw
pointers, act on whichever context is current, and leave argument validity to
the caller. A command calls through a table of addresses that
`Context.create` and `make_current` fill. Calling a command that is not
loaded panics. The reported location is a line of `std/gpu/opengl/gl.dodo`
whose comment names the command. That happens when no
context is current, or when the driver does not provide the command, as macOS
does not for anything after 4.1. `gl.is_loaded("glDispatchCompute")` tests a
command first. `context.proc_address(name)` returns the address of an
extension function the binding does not cover.

The binding is generated by `scripts/generate_opengl.py` from the registry's
`gl.xml`. Rerun it with `--write` to regenerate it, or with `--check` to
verify the checked-in file.

## Platform notes

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Display | Mesa's surfaceless platform when available, otherwise the default EGL display, which vendor drivers serve headless. | None needed. | A hidden 1×1 window on the interactive desktop. |
| Context | Made current without a surface (`EGL_KHR_surfaceless_context`), or with a 1×1 pbuffer. | A CGL context; GPU preferred, Apple's software renderer otherwise. | A legacy context first, then a versioned one through `wglCreateContextAttribsARB`. |
| Debug contexts | Through `EGL_CONTEXT_OPENGL_DEBUG`. | Ignored. | Through `WGL_CONTEXT_DEBUG_BIT_ARB`. |
| Without a GPU driver | `NoLibrary` without `libEGL.so.1`; install Mesa (`libegl1`, `libegl-mesa0`) for llvmpipe. | Always available. | Windows' built-in renderer stops at OpenGL 1.1, so 3.3 fails with `Unsupported`. |

## Errors

`opengl.Error` and `opengl.error_name(error)`:

| Error | When |
| --- | --- |
| `InvalidInput` | A version OpenGL never defined, such as 3.4, or a target of zero size or above the driver's renderbuffer limit. Also a pixel buffer shorter than `bytes_needed()`. |
| `NoLibrary` | The system's OpenGL or EGL library could not be loaded. |
| `NoDisplay` | No display or device to render on: EGL found none, or Windows has no desktop for the hidden window. |
| `NoConfig` | No pixel format or framebuffer configuration supports OpenGL. |
| `Unsupported` | The driver's version or profile is older than requested, or a target needs OpenGL 3.0 that the context lacks. |
| `System` | Another call failed, such as making the context current. |

## Limits

There is no presentation to a window yet. Frames reach the screen by reading
them back, as shown above. The binding covers the core profile, not
compatibility-profile commands such as `glBegin`, and not extensions. Reach
extensions through `context.proc_address`. OpenGL ES and WebGL are not
supported.
