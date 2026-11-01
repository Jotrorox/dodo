---
title: "Images"
description: "Decode and encode PPM/PAM/PFM, QOI, BMP, PNG, APNG and JPEG XL files into caller storage in any pixel layout, with color information, EXIF/XMP/ICC metadata, animation frames and encoder options, on any target."
section: "Standard library"
order: 160.55
---

`std/image` reads and writes image files with storage you provide: no
allocation, no files and no foreign code, so the same calls work on a
desktop, in WebAssembly and on a microcontroller. Integer formats decode with
integer arithmetic only. Read a file with [std/fs](filesystem.md) (or embed
its bytes), ask for its header, give the decoder pixel storage and scratch,
and decode.

```dodo
import "std/image"
import "std/image/pixels"

header := image.info(&data)?                      // size, layout, color
layout := pixels.Layout.rgba16()                  // what you want to work in
target := pixels.ViewMut.new(&mut storage, header.width, header.height, layout.clone())?
need := image.scratch_size(&data, &layout)?
image.decode(&data, &mut target, &mut scratch[..need])?

source := target.as_view()
options := image.EncodeOptions.new()
length := image.encode(pixels.Format.Png, &source, &options, &mut output, &mut scratch)?
```

`storage` needs `pixels.bytes_needed(width, height, &layout)` bytes, and
`output` needs `image.max_encoded_size(format, &source, &options)`. Large
buffers suit `static mut` storage on small stacks.

## Packages

| Import | Contents |
| --- | --- |
| `std/image` | The unified layer: `detect`, `info`, `metadata`, `icc`, `decode`, `draw`, animation frames, `encode`, file extensions. |
| `std/image/pixels` | `Error`, `Format`, `Layout` (channels, sample type, premultiplication), `View`/`ViewMut`, `Header`, `FrameInfo`. |
| `std/image/color` | `ColorInfo` (CICP code points, gamma, chromaticities, ICC location), `Orientation`, integer sRGB curves. |
| `std/image/convert` | Layout conversion, premultiplication, sRGB and linear light. |
| `std/image/sink` | Where decoders put pixels (`Sink`: `ViewSink`, `CanvasSink`, `NullSink`) and where encoders read them (`Source`: `ViewSource`, `CanvasSource`). |
| `std/image/meta` | `Metadata` of EXIF, XMP and ICC byte ranges, the EXIF orientation, zlib and Brotli payloads. |
| `std/image/shared/binary`, `std/image/shared/scratch` | Bounds-checked fields of file data; typed views of one byte scratch buffer. |
| `std/image/ppm`, `std/image/qoi`, `std/image/bmp`, `std/image/png`, `std/image/jxl` | The codecs, one package per format, with the same shape. |
| `std/image/jxl/encode` | The JPEG XL encoder (lossless modular, or lossy VarDCT). |
| `std/image/ops` | Crop, flip, rotate and orient; resize; palettes and dithering; PSNR/SSIM comparison; histograms; fill and compositing (see [below](#pixel-operations)). |
| `std/image/shared/isobmff` | ISO base media file format boxes (the JPEG XL container). |
| `std/image_alloc` | Owned images and image files: `load`, `save`, `convert`, `orient`, `resize`, `crop` with an explicit allocator (see [below](#owned-images-and-files)). |

`std/graphics/image` is the original Canvas-based API; it forwards to the
codec packages (see [below](#stdgraphicsimage)).

## Formats

| Format | Decodes | Encodes |
| --- | --- | --- |
| Netpbm (`Format.Ppm`) | Binary PGM (P5) and PPM (P6) with any maxval; PAM (P7) with tuple types `GRAYSCALE`, `GRAYSCALE_ALPHA`, `RGB`, `RGB_ALPHA`, `BLACKANDWHITE`(`_ALPHA`), 8 or 16-bit; PFM (`Pf` gray, `PF` RGB, 32-bit float, either byte order) | P5 for gray, P6 for RGB, P7 with alpha, PFM for float sources; 8-bit from 8-bit sources, else maxval 65535 |
| QOI | RGB and RGBA, sRGB or linear | RGB, or RGBA when the source has alpha |
| BMP | 1, 4 and 8-bit palettes, RLE8 and RLE4; 16, 24 and 32-bit with or without bit-field masks; bottom-up or top-down; OS/2 core, V4 and V5 headers with sRGB or an embedded ICC profile | 24-bit, or 32-bit with a V5 header and alpha mask when the source has alpha |
| PNG | Every color type and bit depth, `PLTE`, `tRNS`, Adam7, `cICP`, `iCCP`, `sRGB`, `gAMA`, `cHRM`, `cLLi`, `eXIf`, XMP in `iTXt`; APNG animation | Gray, gray+alpha, RGB or RGBA at 8 or 16 bits, or indexed; stored or deflate levels 1 to 9; any filter; Adam7; `sRGB`, `eXIf`, `iCCP` |
| JPEG XL (`Format.Jxl`) | Bare codestreams and the ISOBMFF container (`jxlc`, split `jxlp`, `Exif`, `xml `, Brotli-compressed `brob` boxes decoded by `meta.decode`); lossless and lossy modular, VarDCT with every block type, progressive passes and LF frames, recompressed JPEG (4:4:4, 4:2:2, 4:2:0, gray), patches, splines, noise, Gaborish and EPF, upsampling, animation with blending and reference frames, up to 4096 extra channels (each decodable on its own, spot colors rendered); output U8, U16 or F32 by bit depth, XYB converted to the file's color encoding; embedded ICC profiles via `image.icc` (lossy files with a profile decode to sRGB) | Lossless modular at efforts 1 to 3: 8 and 16-bit integer or exact 16/32-bit float samples, gray, RGB, alpha; or lossy VarDCT in XYB at a Butteraugli distance (efforts 1 to 9: adaptive quantization, Gaborish, varblocks up to 64x64, chroma from luma; alpha kept lossless); prefix codes (effort 1) or tuned hybrid integers and ANS when smaller (effort 2 and up); embedded RGB and gray ICC profiles (lossy: samples converted to XYB through the profile); bare codestream or container with `Exif`/`xml ` boxes |

ASCII Netpbm (P1 to P3), P4 bitmaps, and BMP JPEG/PNG compression fail with
`Unsupported`.

## Layouts and views

A `pixels.Layout` names the channels (`Gray`, `GrayAlpha`, `Rgb`, `Rgba`), the
sample type (`U8`, `U16`, `F16`, `F32`) and whether color is premultiplied by
alpha. Samples are little-endian; integer samples are normalized (0 is black,
the maximum is full intensity) and float samples are nominally 0.0 to 1.0,
with values outside that range allowed for HDR. Shorthands such as
`Layout.rgba8()`, `rgb16()` and `rgba_f32()` cover the common cases.

`pixels.View` and `ViewMut` borrow rows of pixels with a byte stride, so a view
can describe a crop of a larger buffer (`view.sub(x, y, width, height)`).
`convert.copy` converts one view into another of the same size; 8 to 16 bits
multiplies by 257, 16 to 8 rounds, color to gray uses BT.709 luma, and a
missing alpha is opaque.

## Decoding

`image.info(&data)` reads a `pixels.Header` without decoding: `format`,
`width`, `height`, `native` (the layout that holds the file's samples without
loss), `bit_depth`, `color`, `orientation`, `frames`, `loops`, `animated` and
`progressive`. Native layouts keep everything the file stores: 16-bit PNG,
PAM and PPM samples stay 16-bit, palettes expand to `Rgb8` or `Rgba8`, gray
below 8 bits widens to 8, a PNG `tRNS` color key adds an alpha channel, and
PFM stays `F32`.

`image.decode(&data, &mut target, &mut scratch)` converts into `target`, which
must be at least as large as the image. `image.decode_into(&data, &mut sink,
&mut scratch)` instead hands pixels in the native layout to any `sink.Sink`:

```dodo
pub interface Sink {
    fn begin(&mut self, header: &pixels.Header) -> void!pixels.Error
    fn rect(&mut self, x: u32, y: u32, source: &pixels.View) -> void!pixels.Error
}
```

A decoder calls `begin` once, then `rect` with pieces of the image: rows,
runs of a few dozen pixels, or single pixels for interlaced passes. Every
pixel arrives at least once. `sink.ViewSink` converts into a view,
`sink.CanvasSink` draws onto a [graphics](graphics.md) canvas (also
`image.draw(&data, &mut canvas, x, y, &mut scratch)`), and `sink.NullSink`
validates a file without storing it. A sink of your own can scale, stream to
a display, or feed an encoder.

`image.scratch_size(&data, &layout)` is the scratch a decode needs, computed
from the header before any pixel work: PNG needs two rows of raw image data,
the other formats none. The zlib decoder adds about 40 KiB of stack, the
JPEG XL header walks (also `info` and `scratch_size`) at most about 32 KiB.

Every length and index in a file is checked, so untrusted files never trap.
Errors are `Truncated`, `Malformed`, `Unsupported`, `TooLarge` (a dimension
above `pixels.MAX_DIMENSION` or a size overflow) and `BufferTooSmall`; a
failure part-way through can leave part of the image delivered.

## Color and metadata

`header.color` is a `color.ColorInfo` holding CICP code points (`primaries`,
`transfer`, `matrix`, `full_range`), plus what CICP cannot name: a plain
`gamma`, custom chromaticities, the rendering `intent`, the location of an
embedded ICC profile, and `intensity_target` in nits. `tagged` is true when
the file described its colors; untagged images are taken as sRGB.

PNG maps `cICP` first, then `sRGB` (sRGB transfer and intent), then `gAMA`
and `cHRM` (`set_gamma`, `set_chromaticities`, naming CICP sets where they
match); `iCCP` records the compressed profile's range with `ICC_ZLIB`, and
`cLLi` the maximum content light level. BMP V4/V5 `LCS_sRGB` marks sRGB and an
embedded profile is recorded with `ICC_RAW`. A QOI colorspace byte of 1 and
every PFM file are linear.

`image.metadata(&data)` returns a `meta.Metadata` whose `exif`, `xmp` and `icc`
items are byte ranges of `data` with an encoding (`ENCODING_RAW`,
`ENCODING_ZLIB`, or `ENCODING_BROTLI` for JPEG XL `brob` boxes, whose item
covers the 4-byte inner box type and the Brotli stream); nothing is copied.
`item.bytes(&data)` borrows a raw item and `meta.decode(&data, &item, &mut
output)` copies, inflates or decompresses one with
[`std/compress/brotli`](compression.md#brotli). A decoded EXIF payload
starts at the TIFF header whatever its encoding; for a `brob` box `output`
must also hold the 4-byte offset field that precedes it. Brotli decoding
keeps 256 KiB of workspace on the stack.
`image.icc(&data, &mut output, &mut scratch)` returns the decoded ICC profile
directly; only JPEG XL needs scratch (`image.icc_scratch_size(&data)`). It
is the profile of the decoded pixels: lossy (XYB) JPEG XL files with an
embedded profile decode to sRGB, so `header.color` is sRGB and `image.icc`
returns 0 for them, while `image.metadata` lists the profile and `jxl.icc`
extracts it. PNG
finds `eXIf`, `iTXt` with the keyword `XML:com.adobe.xmp`, and `iCCP` before or
after the image data. `header.orientation` comes from the EXIF orientation tag;
`header.display_width()` and `display_height()` account for it.

## Animation

`header.frames`, `image.frame_count` and `image.frame_info(&data, index)`
(a `FrameInfo` of `numerator / denominator` seconds) describe an animation;
still images have one frame of zero duration. `image.decode_frame(&data,
index, &mut target, &mut scratch)` renders frame `index` the way a viewer
shows it, with `image.frame_scratch_size(&data)` bytes of scratch.

For APNG, frames 0 through `index` are composited onto a transparent canvas
in straight 16-bit RGBA, following each frame's dispose (none, background,
previous) and blend (source, over) operations, and the canvas arrives as one
`Rgba16` rect. Sequence numbers are checked. `decode` always returns the
default image (the `IDAT` image), which may or may not be frame 0; frame
indices follow the animation's own frames.

## Encoding

`image.EncodeOptions` is format-independent: `effort` (1 to 9, the PNG deflate
level, 0 writing stored PNG blocks; lossless JPEG XL effort 1 for 1-2, 2 for
3-6 and 3 for 7-9, lossy JPEG XL the same effort), `lossless` (default true),
`quality` (0 to 100, default 90) and `distance` for lossy encoding, and
`keep_metadata`. JPEG XL is the one format with both: `lossless = false`
writes lossy JPEG XL at `distance` when it is above zero, else at the
Butteraugli distance cjxl derives from `quality` (90 is distance 1, the
visually lossless default; quality 100 stays lossless). The other formats
ignore `quality`, `distance` and `lossless`. Encoders
read a `View` in any layout and pick the closest form the format stores, as
the table above lists; float sources are clamped to 0.0 to 1.0 except in PFM.

- `image.max_encoded_size(format, &source, &options)` bounds the output; for
  PPM, BMP and stored PNG it is exact.
- `image.encode_scratch_size(format, &source, &options)` is the scratch
  `encode` needs: the deflate encoder's buffer and workspace for compressed
  PNG (about 350 KiB), JPEG XL's group planes and histograms plus, at effort
  2 and up, ANS tables and a token buffer (about 0.4 to 4 MiB lossless, 3 to
  9 MiB lossy), nothing otherwise.
- `image.encode_with_metadata(format, &source, &options, &exif, &icc, ...)`
  also writes a TIFF-structured EXIF payload and an ICC profile where the
  format stores them (PNG `eXIf` and `iCCP`; JPEG XL an `Exif` box and the
  embedded profile, left out when it is not an RGB or gray profile matching
  the image, or when lossy encoding cannot convert from it, see below); add
  `image.metadata_size(format, exif.len, icc.len)` to the output size.
- `image.encode_canvas` encodes a graphics canvas, read as 8-bit RGBA.

`image.format_from_extension("photo.png")` and `image.extension(&format)` map
between formats and file names.

## Codec packages

Each codec exposes the same functions, so it can be used without the unified
layer, for example to keep a firmware image small:

```dodo
pub fn matches(data: &[u8]) -> bool
pub fn info(data: &[u8]) -> pixels.Header!pixels.Error
pub fn metadata(data: &[u8]) -> meta.Metadata!pixels.Error
pub fn scratch_size(data: &[u8]) -> usize!pixels.Error
pub fn decode<S: sink.Sink>(data: &[u8], output: &mut S, scratch: &mut[u8]) -> void!pixels.Error
pub struct EncodeOptions { ... pub fn new() -> EncodeOptions }
pub fn max_encoded_size(width: u32, height: u32, layout: &pixels.Layout, options: &EncodeOptions) -> usize!pixels.Error
pub fn encode_scratch_size(width: u32, height: u32, layout: &pixels.Layout, options: &EncodeOptions) -> usize!pixels.Error
pub fn encode(source: &pixels.View, options: &EncodeOptions, output: &mut[u8], scratch: &mut[u8]) -> usize!pixels.Error
pub fn encode_from<S: sink.Source>(source: &S, options: &EncodeOptions, output: &mut[u8], scratch: &mut[u8]) -> usize!pixels.Error
```

`encode_from` reads any `sink.Source`, which loads runs of pixels as straight
8 or 16-bit RGBA; `sink.ViewSource` and `sink.CanvasSource` implement it.

The codec options:

| Package | `EncodeOptions` |
| --- | --- |
| `std/image/ppm` | `kind`: `KIND_AUTO`, `KIND_PGM` (P5), `KIND_PPM` (P6), `KIND_PAM` (P7), `KIND_PFM`. |
| `std/image/qoi` | `channels` (0 for automatic, 3 or 4), `linear` (the colorspace byte). |
| `std/image/bmp` | `alpha`: write 32-bit with alpha when the source has it (default), or always 24-bit. |
| `std/image/jxl/encode` | `lossless` (default true) and `distance` (Butteraugli distance of lossy encoding, default 1.0, 0.05 to 25; `EncodeOptions.lossy(d)`, `distance_from_quality(q)` maps cjxl's 0 to 100 quality), `effort` (lossless 1 to 3, higher values behave like 3: 1 uses prefix codes, 2 and 3 tune the hybrid integer coding per cluster and switch to ANS when that is smaller; lossy 1 to 9: 1 DCT8 with uniform quantization, 2 adds adaptive quantization, inverse Gaborish and chroma from luma, 3 to 6 the varblock search up to 32x32 and per-block quant adjustment, 7 to 9 also 64x64; lossy entropy coding follows the lossless levels, prefix codes at 1, tuned hybrid integers and ANS when smaller from 2), `container` (ISOBMFF boxes instead of a bare codestream), `color` (a `ColorInfo` written as the JPEG XL color encoding), `orientation`. `encode_with_metadata` adds `Exif` and `xml ` boxes and embeds `Metadata.with_icc(exif, xmp, icc)`'s ICC profile (predictively coded as in libjxl; it then replaces `color`; an RGB profile for RGB(A), gray for gray, else `Unsupported`; `icc_fits` checks). Lossy encoding converts the samples to XYB through the profile as a color management system reads it (the AToB lookup table of its rendering intent, else its matrix/TRC model or gray curve; relative colorimetric) and embeds it as libjxl does, so decoders with color management render into it; profiles it cannot convert from give `Unsupported` (`icc_usable(icc, layout, options)` checks both). Size with `max_encoded_size_with_metadata`. |
| `std/image/png` | `level` (0 stored, 1 to 9 deflate), `filter` (`FILTER_NONE` to `FILTER_PAETH`, or `FILTER_ADAPTIVE`), `interlace` (Adam7), `palette` (an indexed PNG of 1 to 8 bits with `tRNS` when an 8-bit source has at most 256 RGBA colors), `srgb_chunk`. |

`std/image/jxl` adds `frame_info` and `decode_frame` for animations, `icc`
for the embedded profile, and the extra channel API below.

### JPEG XL extra channels

A JPEG XL image can carry up to 4096 extra channels: alpha, depth maps,
spot colors, selection masks, CMYK black, CFA, thermal or optional data.
`decode` and `decode_frame` output the color channels and the first alpha
channel; the others are read one at a time:

```dodo
pub fn extra_channel_count(data: &[u8]) -> u32!pixels.Error
pub fn extra_channel_info(data: &[u8], channel: u32) -> ExtraChannelInfo!pixels.Error
pub fn extra_channel_name(data: &[u8], channel: u32, output: &mut[u8]) -> usize!pixels.Error
pub fn decode_extra_channel<S: sink.Sink>(data: &[u8], index: u32, channel: u32, output: &mut S, scratch: &mut[u8]) -> void!pixels.Error
```

The first three read only the image header (no scratch, a few KiB of
stack).

`ExtraChannelInfo` has the `kind` (`ExtraChannelType.Alpha`, `Depth`,
`SpotColor`, `SelectionMask`, `Black`, `Cfa`, `Thermal` or `Optional`),
`bit_depth` and `exponent_bits` (float samples), `dim_shift` (the channel is
coded at 1/2^dim_shift of the image size), `alpha_associated`, `spot_color`
(RGB and solidity), `cfa_channel`, `name_length` and the `layout`
`decode_extra_channel` emits: gray, U8 for depths up to 8 bits, U16 up to
16, F32 for float or deeper samples. `decode_extra_channel` takes the same
scratch as `decode_frame` (`scratch_size`) and delivers displayed frame
`index` of the channel at full image size, upsampled from its `dim_shift`
and composited across animation frames like the color channels, as libjxl
does. Spot colors are also rendered into the color output, as `djxl` does
by default. CMYK images are not converted: the color channels hold C, M
and Y as stored (with the CMYK profile from `icc`) and K is the `Black`
extra channel, as libjxl's decoder returns them without a color management
system.

```dodo
count := jxl.extra_channel_count(&data)?
for channel in 0u32..count {
    info := jxl.extra_channel_info(&data, channel)?
    if info.kind == jxl.ExtraChannelType.Depth {
        jxl.decode_extra_channel(&data, 0, channel, &mut depth_sink, scratch)?
    }
}
```

`std/image/png` adds `frame_info`, `frame_scratch_size` and `decode_frame` for
APNG, `encode_with_metadata` for `eXIf` and `iCCP` chunks, and
`encode_with_buffers` to pass the deflate encoder's buffer and workspace
separately instead of carving them from one scratch slice.

## Pixel operations

`std/image/ops` works on `pixels.View` sources and writes into caller
`pixels.ViewMut` targets of any layout, converting as it goes. 8 and 16-bit
work is integer or fixed-point, so results are identical on every target;
float targets keep values outside 0.0 to 1.0.

| Function | What it does |
| --- | --- |
| `crop`, `crop_mut`, `copy` | A sub-view of a rectangle; a converting copy. |
| `flip_horizontal`, `flip_vertical`, `rotate90`, `rotate180`, `rotate270`, `transpose`, `transverse` | Geometry into a target of the right size; `*_in_place` variants for flips and 180 degrees. |
| `orient(&source, &mut target, &orientation)` | Applies an EXIF orientation (`header.orientation`); `oriented_size` gives the target size. |
| `resize(&source, &mut target, &options, scratch)` | Separable resampling with `Filter.Nearest`, `Box`, `Bilinear`, `CatmullRom`, `Mitchell` or `Lanczos3`; alpha-aware (filters premultiplied), optionally in linear light (`ResizeOptions.linear`); scratch from `resize_scratch_size`. `ResizeSink` resizes rows straight from a decoder. |
| `palette`, `remap`, `nearest_color`, `reduce_depth` | Median-cut palettes of up to 256 colors, remapping with `Dither.None`, `FloydSteinberg` or `Ordered`, and per-channel bit-depth reduction (for example RGB565) with dithering. |
| `measure`, `mse`, `psnr`, `ssim`, `max_difference`, `difference_count`, `equal` | Comparison through straight 16-bit RGBA; SSIM uses BT.709 luma in 8x8 windows. |
| `histogram`, `luma_histogram`, `channel_range`, `mean` | Statistics per channel. |
| `fill`, `fill8`, `composite_over`, `grayscale`, `invert` | Simple adjustments; `composite_over` blends with straight alpha. |

## std/graphics/image

`std/graphics/image` keeps the original API for drawing images onto
canvases: `info` (format, width, height, `has_alpha`), `decode` into a new
canvas, `draw` onto an existing one, `decode_png`/`draw_png` with caller
scratch of `png_scratch_size` bytes (two raw rows; `decode` and `draw` keep
`PNG_SCRATCH_LEN`, 16 KiB, on the stack), and the canvas encoders
`encode_ppm`, `encode_qoi`, `encode_bmp` (24-bit), `encode_png` (stored) and
`encode_png_compressed`, each with its size function. It calls the codec
packages directly with `sink.CanvasSink` and `sink.CanvasSource`, never the
unified layer, so it stays integer-only and builds for targets without an FPU.
Its `Error` is `pixels.Error`. New code should use `std/image`.

## Owned images and files

`std/image_alloc` builds owned images on top of `std/image`: an `Image<A>`
owns its pixels (tightly packed rows in one layout), the decoded file's
header, and copies of its EXIF payload and decoded ICC profile, all in
storage from an explicit allocator. `load` reads a file with
[std/fs](filesystem.md) and decodes it; every buffer a codec or an
operation needs is allocated at exactly the size it asks for and released
when the call returns, also on failure.

```dodo
import "alloc/shared_arena"
import "std/image/ops"
import "std/image_alloc"

storage := [0u8; 4194304]                       // or a static buffer
arena := shared_arena.SharedArena.new(&mut storage)?
photo := image_alloc.load(path, arena.handle())?
photo.orient()?                                  // apply the EXIF orientation
options := ops.ResizeOptions.new(ops.Filter.Lanczos3)
photo.resize(640, 480, &options)?
photo.save_auto(output)?                         // format from the extension
```

The safe functions take a `shared_arena.Handle`, so several images can share
one arena, each from its own `arena.handle()` (see
[sharing an explicit allocator](allocation.md#sharing-an-explicit-allocator)).
The `_in` variants (`new_in`, `decode_in`, `decode_as_in`, `decode_frame_in`,
`load_in`, `load_as_in`, `read_file_in`, `bytes_in`) accept any allocator
capability with `allocate(&self, &Layout)` and an unsafe
`deallocate(&self, Block)`, as `std/collections/vector` does, and are
unsafe for that reason. An arena reclaims released space only when it is
reset; a program that processes many files can build a new arena over the
same storage for each one.

| Function | Result |
| --- | --- |
| `new(width, height, layout, allocator)` | Zeroed pixels (transparent black). |
| `decode(&data, allocator)`, `decode_as(&data, layout, allocator)` | The default image in the file's native layout, or converted. |
| `decode_frame(&data, index, layout, native, allocator)` | Animation frame `index` as `image.decode_frame` renders it. |
| `load(path, allocator)`, `load_as(path, layout, allocator)` | The same for a file; it is read into a buffer released after decoding. |
| `read_file(path, allocator)`, `write_file(path, &data)`, `bytes(size, allocator)` | Owned `Bytes<A>` (`len`, `as_slice`, `as_mut_slice`, `truncate`) and plain file writes. |

An `Image` has `width`, `height`, `layout`, `stride`, `bytes`/`bytes_mut`,
`view`/`view_mut` (a `pixels.View` over its storage for any `std/image`
or `std/image/ops` function), `header` (the decoded file's `pixels.Header`;
`decoded()` is false for `new` images and `format()` is then `none`),
`color`, `orientation`, `bit_depth`, `exif`, `icc`, `set_exif`, `set_icc` and
`clear_metadata`. Operations that change the pixels replace the buffer with
one from the image's own allocator:

| Method | Effect |
| --- | --- |
| `convert(layout)` | Converts the pixels (as `convert.copy` does). |
| `orient()` | Applies the header's orientation; it becomes `Identity` and an EXIF orientation tag is reset to 1 so viewers do not turn the image again. |
| `transform(&orientation)`, `rotate(90 \| 180 \| 270)`, `flip_horizontal()`, `flip_vertical()` | Flips and quarter turns. |
| `crop(x, y, width, height)` | Keeps a rectangle; `Malformed` when it does not fit. |
| `resize(width, height, &options)` | Resamples with `ops.resize` and its filters, in linear light with `options.linear`. |
| `quantize(colors, dither)` | Reduces to a median-cut palette of at most `colors` with `ops.Dither`; the pixels become 8-bit RGBA, returns the palette size. |
| `decode_from(&data, layout, native)`, `decode_frame_from(&data, index, layout, native)` | Decode into an existing image, reusing its buffer when the size matches. |

`copy(allocator)` and `converted(layout, allocator)` make a new image instead.
`encode(format, &options, allocator)` returns the encoded file as `Bytes<A>`,
`encode_into(format, &options, &mut output)` writes into a slice of
`max_encoded_size(format, &options)` bytes, and `save(path, format,
&options)` and `save_auto(path)` (format from the extension, default options)
write a file. With `options.keep_metadata` (the default) the EXIF payload and
ICC profile are written where the format stores them: PNG `eXIf` and `iCCP`,
JPEG XL `Exif` box and embedded ICC profile. `EncodeOptions.palette` writes an indexed PNG when the image has
at most 256 colors, as after `quantize`.

Errors are `image_alloc.Error`: `Image(pixels.Error)`,
`Allocation(error.AllocError)` or `File(fs.FileError)`, so `?` wraps each in
a function returning `image_alloc.Error`; `image_alloc.error_name(&error)`
is a short fixed description. EXIF from Brotli-compressed JPEG XL `brob`
boxes is decompressed. EXIF and ICC payloads that cannot be read (a damaged
chunk or compressed stream) are skipped rather than failing the decode; ICC profiles above 64 MiB and files above
`MAX_FILE_SIZE` (1 GiB) are not loaded.

## The imgtool example

`examples/imgtool` is a command-line tool built on `std/image_alloc`, with
all images in one shared arena over a 512 MiB static buffer:

```sh
dodo build examples/imgtool -o imgtool
./imgtool info photo.png
./imgtool convert photo.png photo.jxl --effort 7
./imgtool convert photo.png small.jxl --quality 80
./imgtool resize photo.jxl thumb.png 320x0 --filter lanczos --linear
./imgtool compare photo.png thumb-back.png
```

| Command | Effect |
| --- | --- |
| `info FILE` | Format, size, native layout, bit depth, color (CICP, gamma, ICC), orientation, frames and metadata items (with the decoded size of zlib and Brotli payloads). |
| `convert IN OUT [--effort N] [--layout L] [--strip] [--quality Q \| --distance D \| --lossy]` | Re-encodes as OUT's format; `L` is `gray8`, `graya8`, `rgb8`, `rgba8` or the same with `16`, `f16`, `f32`. JPEG XL is lossless unless given a quality (0 to 100), a Butteraugli distance (such as `1.0`) or `--lossy` (quality 90). |
| `resize IN OUT WxH [--filter F] [--linear]` | `F` is `nearest`, `box`, `bilinear`, `catmull-rom`, `mitchell` or `lanczos`; a zero side keeps the aspect ratio. |
| `strip IN OUT` | Re-encodes without EXIF and ICC metadata. |
| `orient IN OUT` | Applies the EXIF orientation. |
| `crop IN OUT X Y W H`, `rotate IN OUT 90\|180\|270`, `flip IN OUT h\|v` | Geometry. |
| `quantize IN OUT N [--dither fs\|ordered\|none]` | At most N colors; an indexed PNG for `.png`. |
| `compare A B` | Identical or not, differing pixels, largest 16-bit sample difference, PSNR and SSIM. |
| `frames IN OUTPREFIX [--format F]` | Writes every animation frame as `OUTPREFIX-000.png`, ... |

The output format comes from the file extension (`png`, `apng`, `qoi`,
`bmp`, `ppm`, `pgm`, `pnm`, `pam`, `pfm`, `jxl`). The exit status is 0 on
success, 1 when a file cannot be read, decoded, processed or written, 2 for
a usage error, and 3 when `compare` finds the images differ.

## Limits

Decoders deliver the default image of an animation through `decode`; APNG
and JPEG XL have frames (`decode_frame`). Lossy JPEG XL matches libjxl's
`djxl` within one 8-bit level. Chroma-subsampled VarDCT frames with
adaptive LF smoothing follow current libjxl, which smooths each channel at
its own size (`djxl` 0.12 cannot decode them); subsampled frames with
varblocks larger than 8x8 are invalid and fail `Malformed`, as in libjxl.
The JPEG XL encoder embeds only RGB and gray ICC profiles (no CMYK). Lossy
encoding with a profile needs one it can convert from: an AToB lookup table
of type lutAtoB, lut16 or lut8 (XYZ or Lab connection space), a matrix/TRC
RGB profile, or a gray profile's curve; profiles with float DToB tables,
gray profiles with lookup tables and anything else fail `Unsupported`
(`std/image` leaves them out). The conversion is relative colorimetric,
ignoring white points. Lossy encoding keeps alpha lossless, uses the
default dequantization matrices, natural coefficient orders and the 8x8 DCT
only among the 8x8 transforms (no DCT4, AFV or identity blocks), and
analyzes every group twice (once only in images up to 2048x2048 pixels). JPEG XL scratch follows the image header, not
the frame sizes a file claims: each frame is decoded over the part that
can reach the output, so with W' = width + 1100, H' = height + 1100 and
C = 3 + extra channels (all of them, up to 4096), `scratch_size` stays
within 64 W' H' C + 5 MiB C + 128 MiB. Not covered by that bound: frames
that start left of or above the image (decoded from their origin),
reference frames as far as later patches read them, and frames whose
global squeeze or above-right delta palette needs the whole frame; check
`scratch_size` against your budget for untrusted files. PNG text chunks
other than XMP are skipped, as are `sBIT`, `pHYs` and timestamps. The PNG encoder
writes palettes only for 8-bit sources, and float samples outside 0.0 to 1.0
survive only in PFM.
