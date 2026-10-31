---
title: "Graphics"
description: "Draw shapes, antialiased paths, gradients, bitmap and TrueType text and images into pixel buffers on any target, present them on display panels or through the Display protocol, and test rendering with golden images."
section: "Standard library"
order: 160.5
---

`std/graphics` draws into pixel storage you own: a fixed array, an allocation,
or a display's frame buffer. Everything is computed on the CPU with integer
arithmetic, so the same program produces byte-identical pixels on a laptop, in
WebAssembly and on a Raspberry Pi Pico without an FPU. Nothing is allocated and
no operating system is needed.

The family has five imports, and three more drive display panels on
microcontrollers:

| Import | Contents |
| --- | --- |
| `std/graphics` | `Canvas`, colors, rectangles, drawing, antialiased paths and shapes, gradients, text rendering, the `Display` protocol, band rendering. |
| `std/graphics/font` | Built-in 6x8 and 8x16 bitmap fonts for ASCII and Latin-1. |
| `std/graphics/truetype` | TrueType (`.ttf`, `.ttc`) fonts drawn antialiased at any size. |
| `std/graphics/image` | PPM, QOI, BMP and PNG decoding and encoding into caller buffers. |
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

These calls store their color in whole pixels; the
[antialiased family](#antialiased-drawing) blends partial pixels instead.

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

## Antialiased drawing

The `aa_` calls draw the same shapes with smooth edges. Each pixel is blended
with the color by the fraction of its area the shape covers, computed exactly
from the shape's edges in integer arithmetic, so antialiased images are as
reproducible as the rest. Unlike the integer primitives, they *composite*
over what is already there; a translucent color is scaled by the coverage.

```dodo test
package smooth

import "std/graphics"

@test
fn edges_blend_and_interiors_fill() {
    pixels := [0u8; 32 * 32 * 4]
    canvas := graphics.canvas(&mut pixels, 32, 32, graphics.PixelFormat.Rgba8888)!
    canvas.clear(graphics.BLACK)
    canvas.aa_fill_circle(15, 15, 10, graphics.WHITE)
    canvas.aa_thick_line(2, 30, 29, 22, 3, graphics.rgba(97, 175, 239, 200))

    match canvas.color_at(15, 15) {
        some(color) => assert_eq(color, graphics.WHITE),
        none => assert(false),
    }
    // A pixel on the rim is only partly covered: a shade of gray.
    match canvas.color_at(15, 5) {
        some(color) => assert(graphics.red(color) > 0 && graphics.red(color) < 255),
        none => assert(false),
    }
}
```

| Call | Draws |
| --- | --- |
| `aa_line(x0, y0, x1, y1, color)` | A one-pixel line between pixel centres with square ends; axis-aligned lines cover the same pixels as `line`. |
| `aa_thick_line(x0, y0, x1, y1, width, color)` | A line `width` pixels wide with round ends. |
| `aa_circle` / `aa_fill_circle(cx, cy, radius, color)` | The ring or disk `circle` and `fill_circle` approximate: radius + 1/2 around the centre of pixel (cx, cy). |
| `aa_ellipse` / `aa_fill_ellipse(cx, cy, rx, ry, color)` | The same for ellipses. |
| `aa_round_rect` / `aa_fill_round_rect(x, y, width, height, radius, color)` | Exactly round corners; radius 0 gives the same pixels as `fill_rect`. |
| `aa_fill_polygon(coordinates, color)` | An even-odd polygon through pixel corners, like `fill_polygon`. |
| `fill_path(path, rule, color)`, `fill_outline(outline, rule, color)` | Any [path or outline](#paths-and-outlines). |
| `gradient_rect`, `gradient_path`, `gradient_outline` | The same with a [gradient](#gradients). |

Clipping, dirty rectangles, origins and bands work as for every other call,
and every pixel format is supported: `Mono1` lights a pixel once it is about
half covered.

### Paths and outlines

Path coordinates are fixed point: `graphics.SUBPIXEL` (256) units make one
pixel, so shapes can sit between pixels. The pixel (x, y) spans x * 256 to
(x + 1) * 256, and its centre is x * 256 + 128.

A `graphics.Path` stores commands in caller-owned `i32` words:

```dodo
storage := [0i32; 256]
path := graphics.Path.new(&mut storage)
path.move_to(4 * graphics.SUBPIXEL, 28 * graphics.SUBPIXEL)
path.cubic_to(10 * 256, 0, 22 * 256, 0, 28 * 256, 28 * 256)
path.quad_to(16 * 256, 16 * 256, 4 * 256, 28 * 256)
path.close()
path.circle(16 * 256, 20 * 256, 3 * 256)       // a whole contour
canvas.fill_path(&path, graphics.FillRule.EvenOdd, graphics.hex(0xe5c07b))
```

`move_to`, `line_to`, `quad_to`, `cubic_to` and `close` build contours; a
contour left open is closed by a straight line. `rect`, `round_rect`,
`ellipse` and `circle` each add a closed contour. A move or line takes 3
words, a quadratic curve 5, a cubic 7, `close` 1, a circle or ellipse 60 and
a rounded rectangle 72. Building a path never fails: when a command does not
fit, `overflowed()` turns true and the path draws nothing, so size the
storage for the largest shape and check `overflowed()` in a test.

`FillRule.NonZero` treats every area the contours wind around as inside, so
overlapping contours merge; `FillRule.EvenOdd` makes each nested contour cut
a hole whatever its direction.

A path is one kind of `graphics.Outline`, the interface the rasterizer draws.
Any type with a `pub fn trace(&self, pen: &mut graphics.Pen)` method is an
outline: it describes itself by calling the same five commands on the pen,
and needs no storage at all. The rasterizer calls `trace` several times per
drawing call (once for the bounds, then usually once more to collect its
edges), so it must describe the same shape each time:

```dodo
struct Diamond {
    cx: i32
    cy: i32
    radius: i32

    pub fn trace(&self, pen: &mut graphics.Pen) {
        pen.move_to(self.cx, self.cy - self.radius)
        pen.line_to(self.cx + self.radius, self.cy)
        pen.line_to(self.cx, self.cy + self.radius)
        pen.line_to(self.cx - self.radius, self.cy)
    }
}

shape := Diamond { cx: 16 * 256, cy: 16 * 256, radius: 10 * 256 }
canvas.fill_outline(&shape, graphics.FillRule.NonZero, graphics.WHITE)
```

Curves are flattened to within 1/32 pixel. Path coordinates are clamped to
+/-2^29 subpixels (+/-2^21 pixels) and the `aa_` calls' pixel coordinates to
+/-2^21, which keeps every product within 64 bits. Work is bounded by the
visible part of the shape: rows and columns outside the clip rectangle are
never visited. Within it, the cost follows the edges rather than the area:
coverage is computed only in the pixels an edge passes through, and solid
interiors are filled as whole spans, so an outline or a long thin line costs
about as much as its length. The outline is flattened once into at most 256
straight edges at a time. When more edges cross the visible rows, the rows
are drawn in bands, each tracing the outline again, and a row crossed by more
than 256 edges traces it once per 256 columns, so an outline with thousands
of edges is still drawn faster as several smaller ones.

### Coverage masks

A shape drawn many times can be rasterized once into a coverage mask, one
byte per pixel from 0 (outside) to 255 (fully covered), and composited from
it afterwards:

```dodo
bounds := graphics.outline_bounds(&shape)            // the pixels it touches
mask := [0u8; 32 * 32]                               // at least width * height
graphics.coverage_mask(&shape, graphics.FillRule.NonZero, &mut mask, bounds.x, bounds.y, bounds.width, bounds.height)
canvas.fill_mask(&mask, bounds.width, bounds.height, bounds.x + 40, bounds.y, graphics.WHITE)
```

`coverage_mask(outline, rule, mask, x, y, width, height)` writes the coverage
of the width x height pixels starting at (x, y), in rows of `width` bytes.
`fill_mask(mask, width, height, x, y, color)` and `gradient_mask` blend each
pixel exactly as `fill_outline` and `gradient_outline` would, so a mask moved
by whole pixels draws the same pixels as the outline moved by the same
amount. Coverage depends on where a shape sits within a pixel, so make one
mask per subpixel offset you draw at.

## Gradients

A `graphics.Gradient` paints colors that change across an area:

```dodo
sky := graphics.Gradient.linear(0, 0, 0, 99, graphics.hex(0x203a5c), graphics.hex(0xf4a259))
sky.add_stop(170, graphics.hex(0xbc4b51))
canvas.gradient_rect(0, 0, 160, 100, &sky)

glow := graphics.Gradient.radial(120, 30, 24, graphics.WHITE, graphics.TRANSPARENT)
canvas.gradient_rect(96, 6, 48, 48, &glow)
```

`Gradient.linear(x0, y0, x1, y1, start, end)` runs from `start` at pixel
(x0, y0) to `end` at pixel (x1, y1). `Gradient.radial(cx, cy, radius, inner,
outer)` runs from the centre of pixel (cx, cy) outward. Colors are sampled at
pixel centres, so the end pixels get exactly the end colors. `add_stop(offset,
color)` inserts a color at `offset`, from 0 (start) to 255 (end); a gradient
holds up to `graphics.GRADIENT_STOPS` (8) stops, and two stops at the same
offset make a hard edge. `color_at(x, y)` returns the color a pixel gets.

Interpolation uses premultiplied alpha, so fading to `TRANSPARENT` thins a
color out instead of darkening it. `set_spread(Spread.Repeat)` and
`Spread.Reflect` continue the gradient beyond its end instead of extending
the end colors (`Spread.Pad`). `set_dither(true)` adds a 4x4 ordered dither
when drawing on `Rgb565` and `Mono1` canvases, trading the bands those
formats show for a fine pattern; on `Mono1` a mid gray becomes a checkerboard.
Gradients composite over the canvas like the antialiased calls.

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

### TrueType fonts

`std/graphics/truetype` draws TrueType fonts antialiased at any size. A font
is read in place from its bytes, a file loaded with [std/fs](filesystem.md)
or bytes compiled into the program; nothing is copied or allocated, and each
glyph's outline is decoded straight from the font data while drawing.

```dodo
import "std/fs"
import "std/graphics"
import "std/graphics/truetype"
import "std/platform"

storage := [0u8; 1048576]
workspace := platform.workspace()
report := fs.read_file("DejaVuSans.ttf", &mut storage, &mut workspace)?
font := truetype.Font.new(&storage[..report.read])?

bounds := font.draw(&mut canvas, 8, 8, "Grüße, 50 €", 24, graphics.WHITE)
size := font.measure("Hello", 16)
```

`draw(canvas, x, y, text, size, color)` places the top-left corner of the
first line at (x, y), with `size` the em size in pixels, and returns the area
that `measure(text, size)` reports: the widest line's advance by the line
count times `line_height(size)`. Each line's baseline is `baseline(size)`
pixels below its top. `"\n"` starts a new line, kerning pairs from the
font's `kern` table are applied, and glyphs are placed to 1/256 pixel, so
spacing stays even at small sizes. `draw_gradient` takes a gradient instead
of a color, and `font.glyph(index, x, y, size)` returns a single glyph as a
`graphics.Outline` for `fill_outline`, positioned by its baseline origin in
subpixels.

`Font.new` opens a `.ttf` file or the first font of a `.ttc` collection;
`truetype.font_count(data)` and `Font.from_collection(data, index)` reach the
others. Lower-level lookups work in font units: `glyph_index(code_point)`
(0, the box-shaped `.notdef` glyph, when the font has none), `advance_width`,
`left_side_bearing`, `kerning`, `glyph_box`, `units_per_em`, `ascent`,
`descent` and `line_gap`.

Supported are TrueType outlines, simple and composite glyphs, and character
maps in formats 0, 4, 6 and 12, which cover all of Unicode. Fonts with CFF
outlines (`.otf` files beginning with `OTTO`) return `Error.Unsupported`;
other errors are `Malformed` and `NoSuchFont`. Untrusted fonts are safe:
every offset is checked, composite glyphs are limited in depth and size, and
a damaged glyph draws as much of its outline as is valid.

### Glyph cache

`Font.draw` rasterizes every glyph from its outline each time. Text that is
drawn again, such as labels and counters redrawn every frame, is much faster
through a `truetype.GlyphCache`, which keeps each rasterized glyph as a
coverage mask in storage you provide:

```dodo
cache_storage := [0u8; 65536]
cache := truetype.GlyphCache.new(&font, &mut cache_storage)

cache.draw(&mut canvas, 8, 8, "Score: 1200", 14, graphics.WHITE)
```

`cache.draw` and `cache.draw_gradient` take the same arguments and return the
same area as the `Font` calls. The first time a glyph appears at a size it is
rasterized into the cache; afterwards drawing it is a blend of its mask. On
an Apple M-series machine at `-O2`, repeated 14-pixel text drew about 25
times faster than with `Font.draw` (0.19 µs instead of 5 µs per glyph) and
96-pixel text about 11 times faster (1 µs instead of 11 µs).

The one visible difference is placement: the cache positions glyphs to a
quarter pixel horizontally, where `Font.draw` uses 1/256 pixel, and keeps a
separate mask for each of the four quarter-pixel offsets a glyph is drawn
at. Glyphs that land on a quarter pixel draw exactly as `Font.draw` draws
them.

Any amount of storage works, and more holds more glyphs: 64 KiB holds
several hundred glyphs at 14 pixels or a dozen or so at 96. A full cache
evicts the glyphs it cached longest ago. A glyph whose mask would need more
than a quarter of the storage is drawn from its outline without being
cached, so one huge glyph cannot flush the rest. `len()`, `hits()` and
`misses()` show how well the storage fits the text, and `clear()` empties
the cache. The cache borrows the font, so it cannot outlive it; use one
cache per font.

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
| PNG | Every color type and bit depth (gray, RGB, palette, gray and RGB with alpha; 1 to 16 bits), `tRNS` transparency, Adam7 interlacing | 8-bit RGBA: uncompressed, `png_size` bytes; or compressed, at most `png_compressed_max_size` bytes |

PPM and BMP encoding drop alpha; QOI and PNG keep it. Errors are `Truncated`,
`Malformed`, `Unsupported` (ASCII PPM, compressed BMP, an unknown critical PNG
chunk), `TooLarge`, and `BufferTooSmall`. Every length is checked before use,
so untrusted files are safe to decode; a failure part-way through `draw` can
leave part of the image drawn.

PNG decoding checks every chunk's CRC and the zlib Adler-32, skips unknown
ancillary chunks, and rounds 16-bit samples to 8 bits. Gamma, chromaticity and
color profile chunks are ignored, so samples are taken as sRGB. Unfiltering
needs two rows of raw image data: `decode` and `draw` keep
`image.PNG_SCRATCH_LEN` (16 KiB) bytes of them on the stack and fail with
`BufferTooSmall` for wider rows, such as RGBA wider than 2047 pixels. Pass
your own storage of `image.png_scratch_size(&data)` bytes to `decode_png` or
`draw_png` instead. The zlib decoder adds about 40 KiB of stack either way.

```dodo
scratch := [0u8; 32768]                   // png_scratch_size(&data) or more
image.draw_png(&data, &mut canvas, 0, 0, &mut scratch)?
```

`encode_png` writes stored (uncompressed) deflate blocks with no row filters:
the size is exact and nothing beyond the output buffer is needed, which suits
firmware. `encode_png_compressed` picks a filter for each row and compresses
with [std/compress](compression.md); it borrows `deflate.ENCODER_BUFFER_LEN`
bytes and `deflate.WORKSPACE_LEN` words of storage, about 350 KiB together,
which static storage provides best.

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

There is no floating point and no GPU path: both are deliberate for
predictable results on microcontrollers. Paths can be filled but not yet
stroked, except for the straight `aa_line` and `aa_thick_line`. TrueType
text is not hinted, and there is no text shaping: no ligatures, GPOS
kerning, right-to-left or vertical text, and characters map one to one to
glyphs.

The panel drivers write pixels through the SPI or I2C bus protocol, so a
transfer keeps the processor busy; there is no DMA path for them yet. ST7789
panels are the only colour panels with a driver: ILI9341 and similar
controllers use the same window commands but different setup, and SH1106
OLED controllers lack the SSD1306's addressing mode.
