---
title: "Graphics"
description: "Draw shapes, text and images into pixel buffers on any target, present them on display panels or through the Display protocol, and test rendering with golden images."
section: "Standard library"
order: 160.5
---

`std/graphics` draws into pixel storage you own: a fixed array, an allocation,
or a display's frame buffer. Everything is computed on the CPU with integer
arithmetic, so the same program produces byte-identical pixels on a laptop, in
WebAssembly and on a Raspberry Pi Pico without an FPU. Nothing is allocated and
no operating system is needed.

The family has four imports, and three more drive display panels on
microcontrollers:

| Import | Contents |
| --- | --- |
| `std/graphics` | `Canvas`, colors, rectangles, drawing, text rendering, the `Display` protocol, band rendering. |
| `std/graphics/font` | Built-in 6x8 and 8x16 bitmap fonts for ASCII and Latin-1. |
| `std/graphics/image` | PPM, QOI and BMP decoding and encoding into caller buffers. |
| `std/graphics/fake` | An in-memory `Display` for tests. |
| `std/embedded/display` | The SPI and I2C links that panel drivers talk over. |
| `std/embedded/display/st7789` | ST7789 colour TFT and IPS panels, RGB565 over SPI. |
| `std/embedded/display/ssd1306` | SSD1306 monochrome OLED panels, over I2C or SPI. |

To show a canvas on a desktop, open a window with [std/window](window.md),
which implements the `Display` protocol below like the panel drivers do.

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

A canvas covers `canvas.bounds()` of the display: all of it when the canvas
has the display's size, or a part of it when it is smaller or has an
[origin](#bands-and-origins). Displays show only what lies inside both the
canvas and the display and ignore the rest, the way drawing ignores pixels
outside a canvas. Every display converts other pixel formats, so drawing code
works with any canvas.

## Display panels

The drivers under `std/embedded/display` implement `graphics.Display` for
the small panels found on microcontroller boards. A function such as
`draw_status` above therefore runs unchanged on a panel, and its tests keep
running against `fake.Display`.

| Driver | Panels | Format | Connected over |
| --- | --- | --- | --- |
| `st7789.Display` | 1.14" to 2.4" colour TFT and IPS modules: 240x240, 240x320, 135x240, 240x280, 172x320, 170x320 | `Rgb565` | SPI |
| `ssd1306.Display` | 0.42" to 1.3" monochrome OLED modules: 128x64, 128x32, 96x16, 64x48, 72x40 | `Mono1` | I2C or SPI |

A driver talks to its controller through a `display.Interface`, which sends
command bytes and data bytes. There are two, and a test can pass its own:

| Interface | Wiring |
| --- | --- |
| `display.SpiInterface.new(bus, data_command, select)` | An SPI bus, the panel's D/C pin (low for commands, high for data), and its chip-select pin, or `none` when that is wired low. |
| `display.I2cInterface.new(bus, address)` | An I2C bus and the controller's 7-bit address, such as `ssd1306.ADDRESS` (0x3C). |

An ST7789 panel on a Raspberry Pi Pico:

```dodo
bus := b.pins.spi(board.SPI_CLOCK_PIN, board.SPI_TX_PIN, none, hal.SpiMode.Mode3, 62_500_000)!
data_command := b.pins.output(16)!
select := b.pins.output(board.SPI_SELECT_PIN)!
reset := b.pins.output(20)!
display.reset(&mut reset, &mut b.timer)
wiring := display.SpiInterface.new(bus, data_command, some(select))
lcd := st7789.Display.new(wiring, st7789.panel_240x240(), st7789.Rotation.Deg0, &mut b.timer)!

pixels := [0u8; 240 * 240 * 2]
canvas := graphics.canvas(&mut pixels, 240, 240, graphics.PixelFormat.Rgb565)!
canvas.clear(graphics.BLACK)
graphics.present(&mut lcd, &mut canvas)!
```

`st7789.Display.new` resets the controller by command, selects 16-bit
colour, applies the panel's colour inversion and subpixel order, and turns
the display on, which takes about 300 ms. The panel's memory keeps whatever
it held before, so present a whole frame (or `lcd.fill(color)`) first.
`display.reset` pulses a reset pin wired to a GPIO; a panel whose reset pin is
tied to the board's reset needs no call. An `Rgb565` canvas is sent straight
from its storage; other formats are converted on the way.

`Rotation.Deg90` and `Deg270` turn the picture a quarter turn and swap the
display's width and height: `lcd.size()` reports the result, which the
canvas should match. The controller has memory for 240x320 pixels; a
smaller panel shows part of it, and its `st7789.Panel` holds where. For a
module not in the list, fill in a `Panel` from its datasheet or a working
driver: size, column and row offset, colour inversion (on for most IPS
panels), and BGR order.

An SSD1306 module on the board's I2C pins:

```dodo
bus := b.pins.i2c(board.I2C_SDA_PIN, board.I2C_SCL_PIN, hal.I2C_FAST_HZ)!
oled := ssd1306.Display.new(display.I2cInterface.new(bus, ssd1306.ADDRESS), ssd1306.panel_128x64())!
pixels := [0u8; 128 * 64 / 8]
canvas := graphics.canvas(&mut pixels, 128, 64, graphics.PixelFormat.Mono1)!
```

`ssd1306.Display.new` sets the controller up for the panel, clears it, and
turns it on. The controller stores eight rows per byte, so the driver keeps
a copy of the panel memory (1 KiB): a present changes only the pixels inside
its rectangle and sends only the bytes it touched. Colour canvases light the
pixels whose luma is 128 or more, as a `Mono1` canvas stores them.
[`examples/oled_demo.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/oled_demo.dodo)
is a complete program for a 0.96" module: it finds the display on any free
I2C pin pair and shows an animation, the uptime and the chip's temperature.

| Method | ST7789 | SSD1306 |
| --- | --- | --- |
| `size()`, `format()`, `present(&canvas, dirty)` | yes | yes |
| `set_inverted(on)`, `set_display_on(on)` | yes | yes |
| `fill(color)` | yes | |
| `sleep(&mut delay)`, `wake(&mut delay)` | yes | |
| `set_contrast(level)`, `set_flipped(on)`, `is_lit(x, y)` | | yes |
| `wiring()`, `release()` | the interface | the interface |

Driver methods report bus failures as `hal.Error`; `present`, which the
`Display` protocol defines, reports them as `graphics.Error.Device`.
[`examples/thermometer_board.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/thermometer_board.dodo)
is a complete program: the chip's temperature on an ST7789 panel, with the
screen in
[`examples/thermometer.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/thermometer.dodo)
tested against `fake.Display`. Build it for a board with
`dodo run examples/thermometer_board.dodo --board pico`.

## Bands and origins

A full frame does not always fit in memory: 320x240 at `Rgb565` is 153,600
bytes, more than half an RP2040's RAM. `canvas.set_origin(x, y)` places a
canvas at a point of a larger screen. Drawing calls keep using screen
coordinates, and the canvas stores only the pixels under it, so the same
drawing function renders the whole screen or any part of it.

`graphics.Bands` moves a canvas down the screen one band at a time:

```dodo test
package banded

import "std/graphics"
import "std/graphics/fake"

fn draw_screen(canvas: &mut graphics.Canvas) {
    canvas.clear(graphics.BLACK)
    canvas.fill_circle(60, 60, 50, graphics.YELLOW)
}

@test
fn bands_draw_the_whole_screen() {
    screen_storage := [0u8; 120 * 120 * 2]
    screen := fake.Display.new(&mut screen_storage, 120, 120, graphics.PixelFormat.Rgb565)!
    pixels := [0u8; 120 * 40 * 2]
    band := graphics.canvas(&mut pixels, 120, 40, graphics.PixelFormat.Rgb565)!
    bands := graphics.Bands.new(120)
    for bands.next(&mut band) {
        draw_screen(&mut band)
        graphics.present_all(&mut screen, &mut band)!
    }
    assert_eq(screen.presents(), 3u32)
    match screen.screen().color_at(60, 60) {
        some(color) => assert_eq(color, graphics.YELLOW),
        none => assert(false),
    }
}
```

`bands.next(&mut canvas)` sets the canvas's origin to the next band and
returns true, or returns false after the last band and starts over for the
next frame. The scene is drawn once per band, so use the tallest band the
memory allows. A last band that hangs below the screen is cut off by the
display.

After the first frame, an origin also makes partial updates cheap: move the
canvas over the part of the screen that changes, redraw, and present it.
`examples/thermometer_board.dodo` keeps a canvas for a third of the screen
and redraws only the rows holding its reading.

`bounds()`, `clip()`, `dirty()` and `color_at` use screen coordinates.
`row(y)` and `pixels()` stay relative to the canvas's own top-left pixel,
and `equals` and `diff_bounds` compare canvases from their top-left pixels,
whatever their origins. `set_origin` resets the clip to the new bounds and
empties the dirty rectangle.

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

The panel drivers write pixels through the SPI or I2C bus protocol, so a
transfer keeps the processor busy; there is no DMA path for them yet. ST7789
panels are the only colour panels with a driver: ILI9341 and similar
controllers use the same window commands but different setup, and SH1106
OLED controllers lack the SSD1306's addressing mode.
