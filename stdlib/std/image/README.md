# std/image architecture

Pure Dodo image codecs with caller-supplied storage only: no allocation, no
files, no foreign code. `std/image_alloc` adds owned images and file I/O on
top.

## Packages

| Package | Role |
| --- | --- |
| `std/image` | Unified layer: `info`, `decode`, `draw`, `encode` dispatching on `pixels.Format` with a `match`. |
| `std/image/pixels` | Data model: `Error`, `Format`, `Layout` (`Channels`, `Sample`), `View`/`ViewMut`, `Header`, `FrameInfo`. |
| `std/image/color` | `ColorInfo` (CICP code points + gamma, chromaticities, ICC location), `Orientation`, integer sRGB curves. |
| `std/image/convert` | Layout conversion (integer-only for integer targets), premultiply, sRGB/linear. |
| `std/image/sink` | `Sink` interface (`ViewSink`, `CanvasSink`, `NullSink`) and `Source` interface (`ViewSource`, `CanvasSource`) for encoders. |
| `std/image/meta` | `Metadata` of zero-copy `Item` ranges (EXIF, XMP, ICC), EXIF orientation, zlib payloads. |
| `std/image/shared/binary` | Bounds-checked big/little-endian fields. |
| `std/image/shared/scratch` | Typed views (`u32s`, `f32s`, ...) of one byte scratch buffer. |
| `std/image/shared/isobmff` | ISO base media file format box iteration (JPEG XL container, AVIF). |
| `std/image/ppm`, `qoi`, `bmp`, `png`, `jxl` | Codec layer, one package per format. |
| `std/image/jxl/encode` | JPEG XL encoder: lossless modular (efforts 1 to 3) or lossy VarDCT in XYB at a Butteraugli distance (efforts 1 to 9); prefix codes. |
| `std/image/ops` | Crop, flip/rotate/orient, resize, quantize/dither, compare, histogram. |
| `std/graphics/image` | The original Canvas-based API, forwarding to the codec packages. |
| `std/image_alloc` | Owned `Image<A>` and `Bytes<A>` in an explicit allocator, `load`/`save` through std/fs, in-place convert/orient/resize/crop/quantize, metadata copies. |

Dependencies point one way: `color` <- `pixels` <- `convert` <- `sink`;
codecs import those (and `meta`, `shared/*`), never `std/image`; `std/image`
imports every codec.

## Codec contract

Every codec package exposes the same shape, so `std/image` can dispatch:

```dodo
pub fn matches(data: &[u8]) -> bool                       // signature check only
pub fn info(data: &[u8]) -> pixels.Header!pixels.Error
pub fn metadata(data: &[u8]) -> meta.Metadata!pixels.Error
pub fn scratch_size(data: &[u8]) -> usize!pixels.Error    // bytes decode() needs
pub fn decode<S: sink.Sink>(data: &[u8], output: &mut S, scratch: &mut[u8]) -> void!pixels.Error

// Animated formats. decode() produces the default image; frames follow
// the animation (APNG: frame 0 may differ from the default image).
pub fn frame_info(data: &[u8], index: u32) -> pixels.FrameInfo!pixels.Error
pub fn frame_scratch_size(data: &[u8]) -> usize!pixels.Error
pub fn decode_frame<S: sink.Sink>(data: &[u8], index: u32, output: &mut S, scratch: &mut[u8]) -> void!pixels.Error

// Encoders.
pub struct EncodeOptions { ... pub fn new() -> EncodeOptions }
pub fn max_encoded_size(width: u32, height: u32, layout: &pixels.Layout, options: &EncodeOptions) -> usize!pixels.Error
pub fn encode_scratch_size(width: u32, height: u32, layout: &pixels.Layout, options: &EncodeOptions) -> usize!pixels.Error
pub fn encode(source: &pixels.View, options: &EncodeOptions, output: &mut[u8], scratch: &mut[u8]) -> usize!pixels.Error
pub fn encode_from<S: sink.Source>(source: &S, options: &EncodeOptions, output: &mut[u8], scratch: &mut[u8]) -> usize!pixels.Error
```

Rules:

- `decode` calls `output.begin(&header)` once, then `output.rect(x, y, view)`
  with pixels in `header.native` layout. Rects may be any size and arrive in
  any order (rows, tiles, single pixels); a later rect may overwrite an
  earlier one (progressive passes). Every pixel arrives at least once.
- Samples in a `View` are little-endian; integer samples are normalized.
- Untrusted input never traps: every length and index is checked and errors
  are `Truncated`, `Malformed`, `Unsupported`, `TooLarge` or
  `BufferTooSmall`. Resource use is bounded by `scratch_size`, which is
  computed from the header before any pixel work.
- Integer and fixed-point arithmetic for 8/16-bit paths so results are the
  same on every target; f32 only where the format is defined in floating
  point (JPEG XL VarDCT/XYB) or for float layouts.
- Encoders accept a `View` in any layout and convert internally (see
  `convert.load_rgba16`/`load_rgba8`); `encode_from` reads any `sink.Source`
  (`std/graphics/image` passes a `CanvasSource`). Option structs hold scalars
  only: a borrowed slice in an options value conflicts with `static mut`
  buffers, so metadata payloads are separate parameters
  (`png.encode_with_metadata`).
- Adding a format (JPEG XL): write the package, then replace the
  `pixels.Format.Jxl` arms in `std/image.dodo` (every `match` there names
  it) with calls into it.
