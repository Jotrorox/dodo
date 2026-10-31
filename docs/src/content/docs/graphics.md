---
title: "Graphics"
description: "Draw shapes, text and images into pixel buffers on any target, present them through the Display protocol, and test rendering with golden images."
section: "Standard library"
order: 160.5
---

`std/graphics` draws into pixel storage you own: a fixed array, an allocation,
or a display's frame buffer. Everything is computed on the CPU with integer
arithmetic, so the same program produces byte-identical pixels on a laptop, in
WebAssembly and on a Raspberry Pi Pico without an FPU. Nothing is allocated and
no operating system is needed.

The family has four imports:

| Import | Contents |
| --- | --- |
| `std/graphics` | `Canvas`, colors, rectangles, drawing, text rendering, the `Display` protocol. |
| `std/graphics/font` | Built-in 6x8 and 8x16 bitmap fonts for ASCII and Latin-1. |
| `std/graphics/image` | PPM, QOI and BMP decoding and encoding into caller buffers. |
| `std/graphics/fake` | An in-memory `Display` for tests. |

To show a canvas on a desktop, open a window with [std/window](window.md),
which implements the `Display` protocol below. Display panel drivers are not
part of the library yet; they will implement the same protocol.

## Quickstart

Save this as `picture.dodo`:

```dodo
package picture

import "std/fs"
import "std/graphics"
import "std/graphics/font"
import "std/graphics/image"
import "std/platform"

fn main() -> i32 {
    pixels := [0u8; 160 * 90 * 4]
    canvas := match graphics.canvas(&mut pixels, 160, 90, graphics.PixelFormat.Rgba8888) {
        ok(value) => value,
        err(_) => {
            return 1
        },
    }
    canvas.clear(graphics.hex(0x1d2330))
    canvas.fill_circle(40, 45, 30, graphics.hex(0xe5c07b))
    canvas.fill_round_rect(80, 20, 64, 50, 8, graphics.hex(0x61afef))
    label := font.medium()
    canvas.text(&label, 96, 37, "Dodo", graphics.WHITE)

    encoded := [0u8; 160 * 90 * 3 + 32]
    length := match image.encode_ppm(&canvas, &mut encoded) {
        ok(count) => count,
        err(_) => {
            return 2
        },
    }
    workspace := platform.workspace()
    match fs.write_file("picture.ppm", &encoded[..length], &mut workspace) {
        ok() => {
            return 0
        },
        err(_) => {
            return 3
        },
    }
}
```

```sh
dodo run picture.dodo
```

The program prints nothing, exits 0, and writes `picture.ppm`: a 160x90 image
with a yellow disk and a blue rounded box labelled "Dodo". Most image viewers
open PPM files, including macOS Preview and GIMP. Delete the file when done.

`pixels` is the canvas's storage; the canvas borrows it and never copies or
grows it. `graphics.canvas` fails only when the storage is too small for the
size and format, or a dimension exceeds `graphics.MAX_DIMENSION`. Drawing calls
never fail: shapes are simply cut off at the canvas edge. Exit 2 means the
encode buffer was too small (`image.ppm_size` gives the exact size), and exit 3
that the file could not be written.

## Draw on any display

Code that draws and presents through the `graphics.Display` protocol works with
anything implementing it. Under `dodo test`, use `std/graphics/fake`:

```dodo test
package dashboard

import "std/graphics"
import "std/graphics/fake"
import "std/graphics/font"

// Written once against graphics.Display, not against one screen.
fn draw_status<D: graphics.Display>(display: &mut D, canvas: &mut graphics.Canvas, label: &graphics.Font, celsius: i32) -> void!graphics.Error {
    canvas.clear(graphics.BLACK)
    light := if celsius > 30 { graphics.RED } else { graphics.GREEN }
    canvas.fill_rect(0, 0, 8, 8, light)
    canvas.text(label, 12, 0, "TEMP", graphics.WHITE)
    return graphics.present(display, canvas)
}

@test
fn hot_readings_turn_the_light_red() {
    screen := [0u8; 64 * 8 * 2]
    display := fake.Display.new(&mut screen, 64, 8, graphics.PixelFormat.Rgb565)!
    pixels := [0u8; 64 * 8 * 2]
    canvas := graphics.canvas(&mut pixels, 64, 8, graphics.PixelFormat.Rgb565)!
    label := font.small()

    draw_status(&mut display, &mut canvas, &label, 35)!
    assert_eq(display.presents(), 1u32)
    match display.screen().color_at(0, 0) {
        some(color) => assert_eq(color, graphics.RED),
        none => assert(false),
    }
}
```

Run it with `dodo test dashboard.dodo`. A display reports its `size()` and
preferred `format()` and shows a canvas with `present(&canvas, dirty)`.
`graphics.present(display, canvas)` sends only the canvas's dirty rectangle and
resets it; if presenting fails, the rectangle is marked dirty again so the next
call retries. `graphics.present_all` sends the whole canvas.

## Canvases and pixel formats

| Format | Bytes per pixel | Layout |
| --- | --- | --- |
| `Rgba8888` | 4 | Red, green, blue, alpha. |
| `Bgra8888` | 4 | Blue, green, red, alpha: Windows DIBs, macOS bitmaps, little-endian X11. |
| `Rgb565` | 2 | 5-6-5 bits, high byte first, as ST7789 and ILI9341 SPI panels expect. |
| `Gray8` | 1 | Luminance. |
| `Mono1` | 1/8 | One bit per pixel, leftmost in the most significant bit, rows padded to whole bytes; set bits are white. SSD1306-style panels. |

`graphics.bytes_needed(width, height, format)` is the storage size for tightly
packed rows. `Canvas.with_stride` accepts rows with padding, such as a window
system's shared buffer. `canvas.pixels()` and `canvas.row(y)` expose the bytes
for presenting or encoding.

Colors are `graphics.Color`, a `u32` holding `0xAARRGGBB`, so they copy into
every call. Build them with `rgb`, `rgba`, or `hex(0xRRGGBB)`, or use constants
such as `graphics.WHITE`. Formats without alpha drop it; `Gray8` and `Mono1`
store `graphics.luma(color)` (`Mono1` lights pixels from 128 up). Drawing
*stores* the color, alpha included. To composite, use `blend_rect` for a
translucent fill, `blend` to draw one canvas over another, or `graphics.over`
for two colors. Reading a pixel back with `color_at` widens `Rgb565` channels
by repeating their high bits.

### Clipping and dirty rectangles

Every drawing call is limited to the clip rectangle, the whole canvas until
`set_clip` narrows it. `clear` fills the clip rectangle. Each call also grows
the dirty rectangle to cover exactly the pixels it wrote, which is what makes
partial updates cheap: a full 240x240 RGB565 frame is 115,200 bytes over SPI,
while a changed label may be a few hundred. `dirty()` reads it, `take_dirty()`
reads and resets it, and `mark_dirty` adds an area changed by other means.

## Drawing reference

| Call | Draws |
| --- | --- |
| `clear(color)` | The clip rectangle. |
| `pixel(x, y, color)` | One pixel. |
| `line(x0, y0, x1, y1, color)` | A one-pixel line including both ends. |
| `rect` / `fill_rect(x, y, width, height, color)`, `fill(rect, color)` | Rectangle outline or area. |
| `round_rect` / `fill_round_rect(x, y, width, height, radius, color)` | Rounded corners; the radius is limited to half the shorter side. |
| `circle` / `fill_circle(cx, cy, radius, color)` | A circle spanning 2 * radius + 1 pixels. |
| `ellipse` / `fill_ellipse(cx, cy, rx, ry, color)` | An axis-aligned ellipse. |
| `polygon` / `fill_polygon(coordinates, color)` | A closed outline or even-odd fill through `x0, y0, x1, y1, ...`. |
| `blend_rect(x, y, width, height, color)` | A translucent rectangle. |
| `blit(source, x, y)`, `blit_region(source, rect, x, y)` | Another canvas, converting its format. |
| `blend(source, x, y)`, `blend_region(...)` | Another canvas composited by its alpha. |
| `text(font, x, y, text, color)` | UTF-8 text; returns the bounds of its character cells. |

Polygons take a flat `[N]i32` of coordinate pairs because a Dodo array of
structs cannot be filled by repetition; `[0i32; 2 * N]` can, so a program can
compute its vertices in a loop.

The geometry is exact and deterministic. A filled shape covers the pixels whose
centres lie inside it: a circle of radius r covers the pixels within r + 1/2 of
its centre, and the matching outline is that disk minus the disk of radius
r - 1, so outlines always lie inside their fills. Lines step one pixel per
column (or per row, when steep) and round half up. Fills use the even-odd rule,
so a pentagram has a hollow centre.

Inputs never trap. Rectangles, circles, text and blits are exact for every
`i32` and `u32` value. Line and polygon vertices are clamped to +/-2^29 and
ellipse radii to 16383, which keeps every intermediate product within 64 bits
and only matters for shapes hundreds of millions of pixels away. Work is
bounded by the visible area: a line between distant points costs at most one
step per visible column.

## Text and fonts

`font.small()` returns 6x8 character cells and `font.medium()` 8x16; both
cover U+0020 through U+00FF, including accented Latin-1 letters. Bind a font
once and pass a reference to each call. `font.medium().scaled(2)` draws each
glyph pixel as a 2x2 block (factors 1 to 64).

```dodo
small := font.small()
size := small.measure("Hello")          // 30 x 8
bounds := canvas.text(&small, 4, 4, "Hello\nworld", graphics.WHITE)
```

`"\n"` returns to the starting x on the next line and `"\r"` is ignored. Only
glyph pixels are drawn; fill `bounds` (or `measure` first) for a background.
Characters outside a font draw as an outlined box the size of the cell.

A `graphics.Font` is a plain description of a bitmap, so you can supply your
own: `graphics.Font.new(glyph_width, glyph_height, first, count, &bitmap)`.
Glyphs are stored for consecutive code points from `first`, each
`glyph_height` rows of `ceil(glyph_width / 8)` bytes with the leftmost pixel in
the most significant bit. Put the bitmap in a `static` array so firmware keeps
it in flash.

## Images

`std/graphics/image` decodes from a byte slice and encodes into a byte buffer;
read and write files with [std/fs](filesystem.md) or embed the bytes.

```dodo
header := image.info(&data)?              // format, width, height, has_alpha
pixels := [0u8; 64 * 64 * 4]
sprite := image.decode(&data, &mut pixels, graphics.PixelFormat.Rgba8888)?
image.draw(&data, &mut canvas, 10, 10)?   // straight onto an existing canvas
```

| Format | Decodes | Encodes |
| --- | --- | --- |
| Netpbm | Binary PPM (P6) and PGM (P5), any maxval, comments | P6, `ppm_size` bytes |
| QOI | RGB and RGBA | RGBA, at most `qoi_max_size` bytes |
| BMP | Uncompressed 1, 4, 8-bit palettes; 16, 24, 32-bit with or without bit-field masks; bottom-up or top-down | 24-bit, `bmp_size` bytes |

PPM and BMP encoding drop alpha; QOI keeps it. Errors are `Truncated`,
`Malformed`, `Unsupported` (ASCII PPM, compressed BMP), `TooLarge`, and
`BufferTooSmall`. Every length is checked before use, so untrusted files are
safe to decode; a failure part-way through `draw` can leave part of the image
drawn.

## Test rendering

Because rendering is integer-only, a rendered image can be compared byte for
byte across machines. `canvas.equals(&other)` compares decoded colors, so two
canvases in different formats can be equal; `canvas.diff_bounds(&other)`
returns the rectangle covering every differing pixel. `fake.Display` keeps its
own copy of the screen, updated only inside each presented rectangle: compare
`display.screen()` with the canvas to catch a dirty rectangle that was too
small, and use `fail_next` to exercise retry paths.

For golden-image tests, render a scene, encode it with `image.encode_qoi`, and
compare it with a checked-in file. The standard library's own goldens live in
`tests/graphics/golden/`; `tests/graphics/golden_test.dodo` shows the pattern,
including regenerating every image with `DODO_UPDATE_GOLDEN=1`.

## Limits

There is no antialiasing, no floating point, and no GPU path: those are
deliberate for predictable results on microcontrollers. Fonts are fixed-width
bitmaps; there is no TrueType rasterizer, PNG codec, or text shaping.
