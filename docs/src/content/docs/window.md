---
title: "Windows"
description: "Open desktop windows on Linux, macOS or Windows, draw into them with std/graphics or the GPU, and handle keyboard and mouse events of all of them in one plain loop."
section: "Standard library"
order: 160.6
---

`std/window` opens desktop windows whose drawing areas are
[std/graphics](graphics.md) canvases. A program opens as many windows as it
needs, such as a main window, dialogs and tool windows, and reads the events
of all of them in one ordinary loop, so there are no callbacks to register.
It uses only what each operating system ships, so there is nothing to
install and no build flag to set:

| System | Window system | Linked |
| --- | --- | --- |
| Linux (x86-64, AArch64) | The X11 protocol, spoken directly over the display socket | libc only |
| macOS (Apple silicon, Intel) | AppKit through the Objective-C runtime | AppKit, QuartzCore, CoreGraphics |
| Windows (x64) | Win32 | user32, gdi32 |

Drawing happens on the CPU, as everywhere in `std/graphics`, and the window
system only shows the result. A GPU can draw into a window too: Vulkan,
OpenGL and Metal present straight into it through its
[native handle](#drawing-with-the-gpu). Wayland desktops run these programs
through XWayland, which they provide by default.

## Quickstart

Save this as `hello_window.dodo`:

```dodo
package main

import "std/graphics"
import "std/graphics/font"
import "std/window"

fn run() -> void!window.Error {
    app := window.App.new()?
    win := app.open(window.Options.new("Hello", 480, 240))?
    // 2 on a Retina display, whose canvas has twice the pixels per side.
    scale := win.scale()
    label := font.medium().scaled(scale)
    for {
        canvas := win.canvas()
        canvas.clear(graphics.hex(0x1d2330))
        canvas.fill_circle(60 * scale as i32, 60 * scale as i32, 30 * scale, graphics.hex(0xe5c07b))
        canvas.text(&label, 20 * scale as i32, 120 * scale as i32, "Press Escape to quit", graphics.WHITE)
        dirty := canvas.take_dirty()
        win.update(dirty)?

        match app.wait_event()? {
            window.Event.Close(_) | window.Event.Quit => {
                return ok()
            }
            window.Event.Key(_, key) => {
                if key.key == window.Key.Escape {
                    return ok()
                }
            }
            _ => {}
        }
    }
}

fn main() -> i32 {
    match run() {
        ok() => {
            return 0
        }
        err(_) => {
            return 1
        }
    }
}
```

```sh
dodo run hello_window.dodo
```

A 480x240 window titled "Hello" opens with a yellow disk and a line of
text. It closes, and the program exits 0, when you press Escape, click the
window's close button or press Command-Q on macOS. Exit 1 means no window
could be opened, for example because the program runs over SSH without X
forwarding.

The program redraws after every event. Programs that animate ask for `Frame`
events instead, which say when the next frame is due;
`examples/window.dodo` in the repository is a bouncing ball that does.

## Apps and windows

A `window.App` is the program's connection to the window system and the
event loop of every window opened on it. A process has one App at a time:
`App.new()` fails with `AlreadyOpen` while another exists. `app.open(options)`
opens a window and returns a `Window`, which draws on its own and does not
borrow the App, so windows can be kept in variables and structs freely.

| Call | Does |
| --- | --- |
| `window.App.new()` | Connects to the window system. |
| `app.open(options)` | Opens and shows a window. |
| `app.wait_event()` | Waits for the next event of any window. |
| `app.poll_event()` | The next event if one is pending, else `none`; never waits. |
| `app.wait_event_timeout(ms)` | Waits at most `ms` milliseconds; `none` when the time passes first. |
| `app.close()` | Closes every window and the connection; dropping the App does the same. |

Each `Window` owns a pixel buffer exactly the size of its drawing area.

| Call | Does |
| --- | --- |
| `win.id()` | The `WindowId` its events carry. |
| `win.canvas()` | Lends the buffer out as a `graphics.Canvas` in `Bgra8888`. It keeps its contents between frames. |
| `win.update(dirty)` | Shows the `dirty` part of the buffer, usually what `canvas.take_dirty()` returned. |
| `win.update_all()` | Shows the whole buffer. |
| `win.request_frame()` | Asks for one `Frame` event when it is time to draw the next frame. |
| `win.frame_interval()` | The display's refresh period as a `time.Duration`, such as 16.7 ms at 60 Hz. |
| `win.set_title(title)`, `win.set_cursor_visible(visible)` | Change the title, or hide the pointer over the window. |
| `win.close()` | Closes the window; dropping it does the same. |

Drawing changes nothing on screen until `update`. After a `Resize` event,
the next `canvas()` has the new size and keeps the old picture in its
top-left corner; the buffer is only ever replaced inside the window's own
calls, never while a canvas borrows it.

A window closed by `close()`, or left over after its App closed, fails with
`Closed`, reports a size of 0 by 0 and lends out an empty canvas.

## Several windows

Every event names its window: each variant of `Event` starts with a
`WindowId`, and `window.event_window(&event)` reads it from any event. Compare
it with `win.id()` to route the event, or match it in a pattern. Ids are
never reused within a process, so a late event of a window you closed cannot
be mistaken for one of a newer window.

```dodo
app := window.App.new()?
main := app.open(window.Options.new("Main", 640, 480))?
tools := app.open(window.Options { title: "Tools", width: 160, height: 320, resizable: false, pixel_buffer: true })?
for {
    event := app.wait_event()?
    from_tools := window.event_window(&event) == tools.id()
    match event {
        window.Event.Close(id) => {
            if id == main.id() {
                return ok()
            }
            tools.close()
        }
        window.Event.MouseButton(_, button) => {
            if from_tools {
                // ... pick a tool ...
            } else {
                // ... use it on the main window ...
            }
        }
        _ => {}
    }
}
```

Open a dialog by calling `app.open` when it is needed and close it, or let
it drop, when it is done; the other windows keep running meanwhile. The
first window appears where the system places new windows, centered on
macOS; later ones cascade from it. `examples/window_palette.dodo` paints on
one window with colors picked in a second.

### Animation

An animation draws once per display refresh. `win.request_frame()` asks for
one `Frame` event; the loop draws when it arrives and asks again:

```dodo
win.request_frame()?
for {
    match app.wait_event()? {
        window.Event.Close(_) | window.Event.Quit => {
            return ok()
        }
        window.Event.Frame(_) => {
            canvas := win.canvas()
            // ... draw the next frame ...
            dirty := canvas.take_dirty()
            win.update(dirty)?
            win.request_frame()?
        }
        _ => {
            // ... handle the event ...
        }
    }
}
```

Frames come `frame_interval()` apart, at a fixed pace however many events
arrive: moving the mouse over the window does not hold frames back, because
a frame that is due comes before any other event. Where the window system
reports the display's refreshes, frames follow them (see the platform
notes); elsewhere a timer at the refresh rate keeps the pace. A frame that
takes longer than one refresh skips the refreshes it missed instead of
making the next frames late. A hidden or minimized window, whose display
stops reporting refreshes, still gets a frame every two intervals. Each
window asks for its own frames, so an animated window and a still one, or two
windows on displays with different refresh rates, can share one loop.

Advance animations by `frame_interval()` per frame, or by the time measured
with `std/time`, rather than by a fixed step: displays refresh at 60, 120,
144 Hz or more, and the same step per frame would run twice as fast on a
120 Hz display.

Stop asking for frames when there is nothing left to animate, so the program
sleeps until the next event.

### Drawing into your own canvas

A window also implements the [`graphics.Display`](graphics.md#draw-on-any-display)
protocol, so drawing code written against `Display` for a fake display or an
SPI panel works with a window unchanged:

```dodo
// The same function draws on a window, a fake display or a panel.
fn show<D: graphics.Display>(display: &mut D, canvas: &mut graphics.Canvas) -> void!graphics.Error {
    canvas.clear(graphics.BLACK)
    canvas.fill_rect(10, 10, 100, 50, graphics.GREEN)
    return graphics.present(display, canvas)
}
```

`show(&mut win, &mut canvas)` copies the dirty part of your canvas into the
window's buffer, converting its pixel format if needed, and shows it. As with
every display, only what lies inside both the canvas's `bounds()` and the
window is shown: a canvas the size of `win.size()` fills the window, while a
smaller one, one placed with [`set_origin`](graphics.md#bands-and-origins), or
one left over from before a resize updates just the part it covers. On a
high-density display `win.size()` is larger than the size you opened the
window with. Drawing into `win.canvas()` avoids the copy and always has the
right size.

## Events

Every variant but `Quit` starts with the `WindowId` of its window, left out
below.

| Event | Meaning |
| --- | --- |
| `Close(id)` | The user asked to close the window: its close button or Alt-F4. The window stays open until you call `close()` or drop it, so you can still ask whether to save. |
| `Resize(id, width, height)` | The drawing area has a new size in pixels. The next `canvas()` has it and keeps the old picture in its top-left corner; redraw and `update_all()`. |
| `Focus(id, focused)` | The window gained or lost keyboard focus. |
| `Key(id, KeyEvent)` | A key went down (`pressed`), auto-repeated (`repeat`), or came up, in the focused window. |
| `Text(id, code_point)` | A character typed through the keyboard layout. |
| `MouseMove(id, x, y)` | The pointer moved inside the drawing area. |
| `MouseButton(id, ButtonEvent)` | `Left`, `Middle`, `Right`, `Back` or `Forward` went down or up at `x`, `y`. |
| `MouseLeave(id)` | The pointer left the drawing area. |
| `Scroll(id, dx, dy)` | A wheel or trackpad scrolled; 120 units per wheel notch, positive `dy` scrolls down. |
| `Frame(id)` | Time to draw the frame `request_frame()` asked for. |
| `Quit` | The user asked the whole application to quit: Command-Q or the Quit menu item on macOS. Other systems close windows one at a time and never send it. |

Coordinates are pixels from the drawing area's top-left corner. While a
button is held, moves keep arriving outside the window, with coordinates that
can be negative or beyond the size.

### Keys and text

A key and the text it types are separate events. `KeyEvent.key` names the
physical key by its place on a US keyboard, whatever the layout: `Key.W` is
the key right of Tab, which types "z" on a French keyboard. That keeps game
controls in place on every layout. `Text` carries what the layout produced,
for text fields: Shift-2 gives `Text(64)` ("@") on a US layout and `Text(34)`
(`"`) on a German one, after the `Key` event for `Digit2`.

Text is reported for printable characters only, not for Enter, Tab,
Backspace or Escape. Shortcut combinations with Control, Alt or the
Command/Windows key type nothing, while AltGr, and Option on macOS, still type
their characters. `KeyEvent.modifiers` tells which modifier
keys were down; `shortcut()` is Command on macOS and Control elsewhere.
`window.key_name(&key)` gives the variant name and `window.key_code(&key)` a
stable number for saving key bindings.

## Size and high-density displays

`Options` sizes are in the window system's own units: points on macOS and
pixels at the system scale on Windows. A 480x240 window therefore looks the
same size on every screen. The canvas and all event coordinates are in
physical pixels, which on a Retina display is twice as many in each direction:
`win.size()` reports 960x480 there.

`win.scale_percent()` is the ratio in percent (100, 150, 200, ...) and
`win.scale()` the nearest whole factor. Multiply your own sizes by it, and
scale bitmap fonts with `font.medium().scaled(win.scale())`, to keep them
readable. Nothing is scaled behind your back. X11 has no system-wide scale, so
there both units are pixels and the scale is always 100.

Pass `resizable: false` in the options to keep the size fixed; otherwise
handle `Resize`.

## Drawing with the GPU

A window can be the target of a GPU API instead of, or as well as, a canvas.
`win.native_handle()` returns the window system's own objects, which is what
Vulkan, EGL, WGL, NSOpenGL and Metal take to create a surface:

| System | `window.NativeHandle` | Use it with |
| --- | --- | --- |
| Linux | `Xcb(connection, window, screen, visual)`: a libxcb connection and the X11 window id | `vkCreateXcbSurfaceKHR`, EGL's XCB platform |
| macOS | `AppKit(window, view)`: the `NSWindow` and its content `NSView` | `NSOpenGLContext`; `win.metal_layer()` for Metal and `VK_EXT_metal_surface` |
| Windows | `Win32(hwnd, instance)`: the `HWND` and the program's `HINSTANCE` | `vkCreateWin32SurfaceKHR`, WGL |

Open the window with `window.Options.gpu(title, width, height)` when only the
GPU draws into it. That window has no pixel buffer: `canvas()` lends out an
empty 0x0 canvas, `update()` fails with `Unsupported`, and nothing is copied
on resize. `win.has_pixel_buffer()` tells the two kinds apart. Events, frame
pacing with `request_frame()`, `size()` and `scale()` work the same in both,
and one App can have windows of both kinds.

The simplest way to draw is [std/gpu](gpu.md#surfaces): a `gpu.Surface`
made from the handle presents frames on Metal, Vulkan or OpenGL, whichever
the machine has, as
[`examples/gpu_window.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/gpu_window.dodo)
shows. The raw APIs take the handle as well;
[std/gpu/opengl](opengl.md#drawing-into-a-window) takes it directly:

```dodo
app := window.App.new()?
win := app.open(window.Options.gpu("Triangle", 480, 320))?
native := win.native_handle()?
context := opengl.Context.create_for_window(&native, opengl.Options.default())?
win.request_frame()?
for {
    match app.wait_event()? {
        window.Event.Resize(_, _, _) => {
            context.update_surface()
        }
        window.Event.Frame(_) => {
            // ... draw into framebuffer 0, win.width() by win.height() ...
            context.swap_buffers()?
            win.request_frame()?
        }
        _ => {}
    }
}
```

Three examples in the repository draw a spinning triangle this way:
[`opengl_window.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/opengl_window.dodo),
[`vulkan_window.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/vulkan_window.dodo),
which makes a surface for each system and rebuilds its swapchain on resize
(see [Vulkan](vulkan.md#drawing-into-a-window)),
and [`metal_window.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/metal_window.dodo)
for macOS.

The GPU's drawing area is the window's size in pixels, `win.size()`, which on
a Retina display is twice the size the window was opened with. After a
`Resize` event, resize the swapchain or drawable to match.

### Each system's catch

**Linux.** `std/window` speaks X11 over its own socket, but Vulkan and EGL
need a libxcb connection. The first `native_handle()` call therefore loads
`libxcb.so.1`, which every X11 installation has, and opens a second
connection to the same X server. X11 window ids are valid on every
connection, so the GPU presents into the window through it. Programs that
never ask for a handle still need nothing beyond libc. Every window of the App
shares that connection, which closes with the App; without libxcb the call
fails with `NoDisplay`.

**macOS.** `win.metal_layer()` makes a `CAMetalLayer` the backing layer of
the window's view and returns its address. Wrap it with
`metal.MetalLayer.from_object` for [Metal](metal.md#presenting-to-a-window),
or pass it to `vkCreateMetalSurfaceEXT` for Vulkan through MoltenVK.
`std/window` keeps the layer's `contentsScale` and `drawableSize` at the
window's scale and size in pixels, also across resizes. Only windows opened
with `Options.gpu()` have one, since a pixel-buffer window's layer shows the
buffer. Everything here, like the window itself, belongs to the main thread.

**Windows.** The window class has its own device context (`CS_OWNDC`), as
WGL wants, and a window keeps the first pixel format it is given.

The handle is plain addresses and ids that stay valid until the window
closes. Destroy every surface, context or swapchain made from it first; local
variables are dropped in reverse order, so declaring the window before the
GPU objects does that.

## Platform notes

| | Linux (X11) | macOS | Windows |
| --- | --- | --- | --- |
| Finding the display | `DISPLAY`, then the cookie for it in `XAUTHORITY` or `~/.Xauthority`. Local displays use the Unix socket, `host:N` uses TCP, so SSH X forwarding works. | The logged-in GUI session. | The interactive desktop. |
| Threads | Any one thread, the one that created the App. | The main thread only; elsewhere `App.new` fails with `Unsupported`. | The thread that created the App must open the windows and read their events. |
| Text input | Latin-1 and Unicode keysyms, Shift, Caps Lock, NumLock and AltGr. Dead keys and compose sequences type nothing. | Characters from the layout, Option combinations included. No input method or dead keys. | Characters from the layout, including dead keys and surrogate pairs. |
| While resizing | Events keep flowing. | AppKit holds the event loop until the mouse is released, then reports the final size. | Windows holds the event loop until the mouse is released, then reports the size. |
| Frame timing | The Present extension reports each refresh of each window; servers without it fall back to a 60 Hz timer. `frame_interval()` is measured from Present after about half a second. | A display link per window reports each refresh on macOS 14 and later; older systems use a timer at the screen's refresh rate. Programs are exempt from App Nap while a window waits for a frame. | The compositor's (DWM's) refresh times; a high-resolution timer keeps the waits precise. |

## Errors

`window.Error` and `window.error_name(error)`:

| Error | When |
| --- | --- |
| `NoDisplay` | No window system to connect to: no `DISPLAY` on Linux, no desktop session. Also `native_handle()` on Linux without libxcb. |
| `AlreadyOpen` | Another App exists; a process has one at a time. |
| `Closed` | The window or its App was closed, or the connection to the window system ended. |
| `InvalidInput` | A zero size, a size above `window.MAX_SIZE` (16384), or an oversized title. |
| `OutOfMemory` | The pixel buffer could not be allocated. |
| `Protocol` | The X server refused the connection (usually a missing or wrong cookie) or sent something malformed. |
| `Unsupported` | The X server's default visual is not 24-bit true color, or macOS was used off the main thread. Also `update()` on a window without a pixel buffer, and `metal_layer()` on other systems or on a window with one. |
| `System` | Another operating-system call failed. |

## Limits

Windows have no owner relationships yet: a dialog is an ordinary window,
not kept above the window it belongs to or modal to it, and there are no
borderless popups or ways to place a window. There is no clipboard, no
cursor shapes other than hiding the pointer (`set_cursor_visible`), and no
input methods for Chinese, Japanese or Korean text. A canvas is copied to
the window system on every `update`; for drawing without that copy, use the
GPU. Native Wayland and drawing straight to the screen without a desktop
(DRM/KMS) are not implemented yet.
