---
title: "Windows"
description: "Open a desktop window on Linux, macOS or Windows, draw into it with std/graphics, and handle keyboard and mouse events in a plain loop."
section: "Standard library"
order: 160.6
---

`std/window` opens a desktop window whose drawing area is a
[std/graphics](graphics.md) canvas. Events are read in an ordinary loop, so
there are no callbacks to register. It uses only what each operating system
ships, so there is nothing to install and no build flag to set:

| System | Window system | Linked |
| --- | --- | --- |
| Linux (x86-64, AArch64) | The X11 protocol, spoken directly over the display socket | libc only |
| macOS (Apple silicon, Intel) | AppKit through the Objective-C runtime | AppKit, QuartzCore, CoreGraphics |
| Windows (x64) | Win32 | user32, gdi32 |

Drawing happens on the CPU, as everywhere in `std/graphics`, and the window
system only shows the result. Wayland desktops run these programs through
XWayland, which they provide by default.

## Quickstart

Save this as `hello_window.dodo`:

```dodo
package main

import "std/graphics"
import "std/graphics/font"
import "std/window"

fn run() -> void!window.Error {
    win := window.open(window.Options.new("Hello", 480, 240))?
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

        match win.wait_event()? {
            window.Event.Close => {
                return ok()
            }
            window.Event.Key(key) => {
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
text. It closes, and the program exits 0, when you press Escape or click the
window's close button. Exit 1 means no window could be opened, for example
because the program runs over SSH without X forwarding.

The program redraws after every event. Programs that animate use
`wait_event_until_frame` instead, which also says when the next frame is due;
`examples/window.dodo` in the repository is a bouncing ball that does.

## The main loop

A `Window` owns a pixel buffer exactly the size of its drawing area.

| Call | Does |
| --- | --- |
| `win.canvas()` | Lends the buffer out as a `graphics.Canvas` in `Bgra8888`. It keeps its contents between frames. |
| `win.update(dirty)` | Shows the `dirty` part of the buffer, usually what `canvas.take_dirty()` returned. |
| `win.update_all()` | Shows the whole buffer. |
| `win.wait_event()` | Waits for the next event. |
| `win.poll_event()` | The next event if one is pending, else `none`; never waits. |
| `win.wait_event_timeout(ms)` | Waits at most `ms` milliseconds; `none` when the time passes first. |
| `win.wait_event_until_frame()` | Waits for the next event or the next frame; `none` when it is time to draw. |
| `win.frame_interval()` | The display's refresh period as a `time.Duration`, such as 16.7 ms at 60 Hz. |

Drawing changes nothing on screen until `update`. The canvas borrows the
window, so its last use must come before the next event call; the borrow
checker enforces that. This matters because a resize replaces the buffer
inside the event call.

### Animation

An animation draws once per display refresh. `wait_event_until_frame()`
returns each event as it arrives and `none` when the next frame is due, so
the loop handles events until `none`, then draws:

```dodo
for {
    match win.wait_event_until_frame()? {
        some(window.Event.Close) => {
            return ok()
        }
        some(_) => {
            // ... handle the event ...
        }
        none => {
            canvas := win.canvas()
            // ... draw the next frame ...
            dirty := canvas.take_dirty()
            win.update(dirty)?
        }
    }
}
```

Frames are due `frame_interval()` apart, at a fixed pace however many events
arrive: moving the mouse over the window does not hold frames back. Where
the window system reports the display's refreshes, frames follow them (see
the platform notes); elsewhere a timer at the refresh rate keeps the pace. A
frame that takes longer than one refresh skips the refreshes it missed
instead of making the next frames late. A hidden or minimized window, whose
display stops reporting refreshes, still gets a frame every two intervals.

Advance animations by `frame_interval()` per frame, or by the time measured
with `std/time`, rather than by a fixed step: displays refresh at 60, 120,
144 Hz or more, and the same step per frame would run twice as fast on a
120 Hz display.

Call `wait_event()` when there is nothing left to animate, so the program
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

| Event | Meaning |
| --- | --- |
| `Close` | The user asked to close the window: its close button, Alt-F4, or Command-Q. The window stays open until you call `close()` or drop it, so you can still ask whether to save. |
| `Resize(width, height)` | The drawing area has a new size in pixels. The buffer already has it and keeps the old picture in its top-left corner; redraw and `update_all()`. |
| `Focus(focused)` | The window gained or lost keyboard focus. |
| `Key(KeyEvent)` | A key went down (`pressed`), auto-repeated (`repeat`), or came up. |
| `Text(code_point)` | A character typed through the keyboard layout. |
| `MouseMove(x, y)` | The pointer moved inside the drawing area. |
| `MouseButton(ButtonEvent)` | `Left`, `Middle`, `Right`, `Back` or `Forward` went down or up at `x`, `y`. |
| `MouseLeave` | The pointer left the drawing area. |
| `Scroll(dx, dy)` | A wheel or trackpad scrolled; 120 units per wheel notch, positive `dy` scrolls down. |

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

## Platform notes

| | Linux (X11) | macOS | Windows |
| --- | --- | --- | --- |
| Finding the display | `DISPLAY`, then the cookie for it in `XAUTHORITY` or `~/.Xauthority`. Local displays use the Unix socket, `host:N` uses TCP, so SSH X forwarding works. | The logged-in GUI session. | The interactive desktop. |
| Threads | Any one thread. | The main thread only; elsewhere `open` fails with `Unsupported`. | The thread that opened the window must read its events. |
| Text input | Latin-1 and Unicode keysyms, Shift, Caps Lock, NumLock and AltGr. Dead keys and compose sequences type nothing. | Characters from the layout, Option combinations included. No input method or dead keys. | Characters from the layout, including dead keys and surrogate pairs. |
| While resizing | Events keep flowing. | AppKit holds the event loop until the mouse is released, then reports the final size. | Windows holds the event loop until the mouse is released, then reports the size. |
| Frame timing | The Present extension reports each refresh; servers without it fall back to a 60 Hz timer. `frame_interval()` is measured from Present after about half a second. | A display link reports each refresh on macOS 14 and later; older systems use a timer at the screen's refresh rate. Animating programs are exempt from App Nap until they call `wait_event()`. | The compositor's (DWM's) refresh times; a high-resolution timer keeps the waits precise. |

## Errors

`window.Error` and `window.error_name(error)`:

| Error | When |
| --- | --- |
| `NoDisplay` | No window system to connect to: no `DISPLAY` on Linux, no desktop session. |
| `AlreadyOpen` | Another window is open; a process has one at a time. |
| `Closed` | The window was closed, or the connection to the window system ended. |
| `InvalidInput` | A zero size, a size above `window.MAX_SIZE` (16384), or an oversized title. |
| `OutOfMemory` | The pixel buffer could not be allocated. |
| `Protocol` | The X server refused the connection (usually a missing or wrong cookie) or sent something malformed. |
| `Unsupported` | The X server's default visual is not 24-bit true color, or macOS was used off the main thread. |
| `System` | Another operating-system call failed. |

## Limits

One window per process. There is no clipboard, no cursor shapes other than
hiding the pointer (`set_cursor_visible`), no input methods for Chinese,
Japanese or Korean text, and no GPU path: frames are copied to the window
system on every `update`. Native Wayland and drawing straight to the screen
without a desktop (DRM/KMS) are not implemented yet.
