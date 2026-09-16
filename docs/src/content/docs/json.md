---
title: "JSON encoding and decoding"
description: "Decode JSON into checked structs, derive encoding, and inspect borrowed JSON without an allocator."
section: "Standard library"
order: 147
---

Import `std/encoding/json` and add `@derive(Json)` to a struct to encode and
decode its fields. JSON types are checked during decoding, and ordinary Dodo
field access remains statically typed. The module is portable and needs no
allocator, operating system, or runtime reflection.

## Quickstart

Save this as `json_start.dodo`:

```dodo test
package json_start
import "std/encoding/json"
import "std/bytes"

@derive(Json)
struct Person {
    name: json.String
    age: u8
    nickname: Option<json.String>
}

fn round_trip() -> void!json.Error {
    source := b"{\"name\":\"Ada\",\"age\":36,\"nickname\":null}"
    person := json.decode::<Person>(source)?
    assert_eq(person.age, 36u8)

    storage := [0u8; 128]
    written := json.to_slice(&person, &mut storage)?
    assert(bytes.equal(&storage[..written], source))
    return ok()
}

fn main() -> i32 {
    match round_trip() {
        ok() => {
            return 0
        },
        err(_) => {
            return 1
        },
    }
}
```

```sh
dodo run json_start.dodo
```

Expected output: none; exit 0 confirms the typed round trip. Exit 1 means decoding
or encoding failed. Increase output capacity when handling `BufferFull`; reject
malformed input or inspect its error before retrying. The complete
[`examples/json.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/json.dodo)
also constructs a struct with `json.String.from_str("Grace")`.

## Structs and field names

`@derive(Json)` generates `encode_json` and `decode_json` methods. It supports
booleans, signed and unsigned integers, finite floating-point values, `&str`,
`json.String`, nested derived structs, `Option<T>`, and fixed arrays with literal
lengths of at most 4096 elements. Unsupported field types produce a compile-time diagnostic. User methods
can implement the same structural contract for custom representations.

```dodo
@derive(Json)
@json_deny_unknown
struct Account {
    @json_name("displayName")
    display_name: json.String
    visits: u32
    active: bool
    nickname: Option<json.String>
}
```

Field names match exactly, including case, after JSON string escapes are decoded.
`@json_name("displayName")` applies the same name in both directions. Unknown
members are ignored by default; `@json_deny_unknown` rejects them. Required fields
must be present and have the declared type. `Option<T>` accepts a missing member
or JSON `null` as `none`; `some(value)` decodes the underlying type. Encoding
`none` emits `null`. Fixed arrays require exactly their declared element count.
Decoding does not coerce strings into numbers, numbers into booleans, or `null`
into a required field's zero value.

Integer conversion checks the destination range and preserves all 64 bits.
An integer field rejects fractions and exponent notation, even if their
mathematical value is integral. Floating-point conversion rejects overflow;
encoding rejects NaN and infinities. Generic derived structs, field defaults,
flattening, and automatic enum derivation are not currently supported.

## Entry points and output

The generic `encode` and `decode` functions accept types implementing the JSON
method contract: derived structs, `json.Value`, `json.String`, and custom codec
types. Bare primitive roots such as `u64` and `bool` do not implement those
methods. Decode them through `json.parse` and a checked `Value` accessor; encode
them with the corresponding `Encoder` method. Primitive struct fields work
directly through derivation.

| Operation | API |
| --- | --- |
| Decode a struct from bytes | `json.decode<T>(input) -> T!json.Error` |
| Decode a struct from text | `json.decode_str<T>(input) -> T!json.Error` |
| Decode an inspected value | `json.from_value<T>(value) -> T!json.Error` |
| Inspect JSON dynamically | `json.parse(input) -> json.Value!json.Error` |
| Inspect a language string | `json.parse_str(input) -> json.Value!json.Error` |
| Encode to caller-owned bytes | `json.to_slice(&value, &mut storage) -> usize!json.Error` |
| Encode through a byte writer | `json.encode(&value, &mut writer) -> usize!json.Error` |
| Encode readable output | `json.encode_pretty(&value, &mut writer) -> usize!json.Error` |
| Encode readable output into bytes | `json.to_slice_pretty(&value, &mut storage) -> usize!json.Error` |
| Validate without retaining a view | `json.validate(input) -> void!json.Error` |
| Read a document into caller-owned bytes | `json.parse_reader(&mut reader, &mut storage) -> json.Value!json.Error` |
| Decode a document from a reader | `json.decode_reader<T, R>(&mut reader, &mut storage) -> T!json.Error` |
| Remove insignificant whitespace | `json.compact(input, &mut writer) -> usize!json.Error` |
| Format JSON text for readability | `json.pretty(input, &mut writer) -> usize!json.Error` |

Writer output uses the portable [byte I/O](io.md) structural contract. A fixed
buffer keeps capacity explicit; an existing growable writer can supply separately
managed storage. Output functions report the number of bytes written on success.
Failure can leave an output prefix, so do not treat partially written JSON as a
complete document. There is no hidden allocation or automatic retry.

Reader helpers read through EOF into the supplied storage. When storage fills,
the reader consumes one extra byte to distinguish an exact fit from exhaustion.
An oversized document returns `BufferFull`; returned views borrow the storage,
not the reader. These helpers handle a complete document, rather than a sequence
of independent JSON values on a live stream.

For multiple values already held in a buffer, `json.Decoder.new(input)` or
`json.Decoder.from_str(input)` creates a cursor. `next()` returns
`Option<json.Value>!json.Error`: `some(value)` for a document and `none` at EOF.
Documents must be separated by JSON whitespace, so newline-delimited JSON works
without a special line parser. Use or drop each returned view before advancing
the decoder. `position()` reports the byte cursor; a malformed document leaves
the cursor unchanged and subsequent calls return the same error.

## Borrowed values and strings

`Value` and decoded `json.String` retain checked shared borrows of their input.
Keep the input alive and unchanged while using them. A decoded struct containing
either view has the same lifetime dependency. Decoding an object does not build
an owned heap tree; array/object navigation scans the validated source. Repeated
lookups can therefore repeat work. Duplicate-name validation uses constant extra
storage and compares earlier members; its work grows quadratically with the
number of object members. Apply application input limits to large external
documents.

`json.String` represents a complete JSON string, including escaped content.
`equals("text")` compares its decoded contents; `equal(&other)` compares two JSON
strings. `decoded_len()` gives the required UTF-8 output byte count.
`decode(&mut storage)` unescapes into caller-owned storage and returns a checked
`&str` borrowing that destination. `String.from_str(value)` wraps an existing
language string for encoding without copying it.

`as_str()` returns a borrowed language string only when no JSON escapes need
decoding. It returns `EscapedString` for escaped content, rather than returning
raw escape sequences as text. The same restriction applies when deriving a
field of type `&str`; use `json.String` for arbitrary external JSON strings.
`raw()` exposes the representation's content without surrounding quotes, and
`is_escaped()` identifies encoded string content. `into_str()` consumes the
string wrapper while transferring its input borrow; it likewise rejects escapes.

## Inspecting JSON without a struct

`Value.kind()` identifies the JSON kind. Fallible conversions `as_bool()`,
`as_i64()`, `as_u64()`, `as_f64()`, `as_string()`, and `as_str()` check it;
`is_null()` recognizes null explicitly. `get(name)` looks up an optional object
member, while `require(name)` reports a missing field. `at(index)` selects an
array element. `len()` reports an array or object's member count. Array and
object iterators expose members without allocating a collection.

`pointer("/users/0/name")` navigates an RFC 6901 JSON Pointer. An empty pointer
selects the current value; `~0` represents a literal `~` and `~1` represents a
literal `/` in a member name. Array indexes use decimal digits without leading
zeros. Missing members/indexes return `MissingField`; malformed paths return
`Syntax`, and traversal through a scalar returns `TypeMismatch`. URI fragment
pointers beginning with `#` are not supported.

The array token `-` denotes the nonexistent append position and returns
`MissingField`; it never mutates the document.

`raw()` returns the validated source representation and `position()` reports
its byte offset in the original document. Dynamic inspection is useful for
unknown schemas; derived structs are the shorter path when fields are known.
`into_raw()`, `into_string()`, and `into_str()` consume a value view and transfer
its input borrow into the result, which is useful when returning extracted data
from a helper. `into_str()` has the same escaped-string restriction as `as_str()`.

## Validation and errors

Parsing consumes one complete JSON document. It rejects trailing content,
comments, trailing commas, duplicate object names, leading-zero numbers,
malformed UTF-8, unescaped control characters, invalid Unicode escapes, and
unpaired surrogate halves.
Duplicate names are rejected even when one uses escapes, such as `"age"` and
`"\u0061ge"`. Nested containers have a maximum depth of 128. String equality uses
decoded Unicode content without normalization or case folding.

Errors distinguish syntax failures, unexpected JSON kinds, missing/duplicate/
unknown fields, numeric range failures, capacity exhaustion, I/O errors, depth
limits, trailing data, and escaped-string borrowing. `Error.position` is a byte
offset in the input for parsing and conversion errors; writer failures report
the number of output bytes already emitted. Handle errors with `match`, propagate
them with `?`, or use `!` only when failure should terminate the program.
See [Results](patterns-and-results.md).

Custom `encode_json<W>` methods receive `&mut json.Encoder<W>` and return
`void!json.Error`; custom `decode_json` methods consume a `json.Value` and return
`Self!json.Error from(value)`. `json.from_value<T>(value)` likewise consumes the
view while retaining the input's lifetime in borrowed output fields. The encoder
supplies `begin_object`, `key`, `end_object`,
`begin_array`, `end_array`, `null`, `boolean`, `signed`, `unsigned`, `floating`,
`string`, `json_string`, and `value`. Its state machine rejects incomplete
members and invalid token order. Custom object encoders must supply unique key
names. `raw` validates a complete encoded value before insertion. A failed
encoder stays failed, and `finish()` checks that one complete value was written.

The parser, derivation, and public entry points have native execution tests at
O0 and O3 plus freestanding object-emission tests for
`wasm32-unknown-unknown` and `thumbv6m-none-eabi`. Object emission verifies
portability of generated code; it does not execute either target.

## API design references

The typed entry points and generated struct conversions draw on
[Serde's derive model](https://serde.rs/derive.html) and
[Serde JSON's typed representation](https://docs.rs/serde_json/latest/serde_json/).
Explicit field naming follows the same practical need addressed by
[Serde field attributes](https://serde.rs/field-attrs.html). Strict UTF-8,
case-sensitive member names, and exact array lengths follow the choices described
in [Go's JSON v2 documentation](https://pkg.go.dev/encoding/json/v2).
[JSON for Modern C++](https://json.nlohmann.me/home/design_goals/) supplied another
reference for concise everyday operations and testing exceptional behavior.
Dodo adapts these ideas to checked borrows, explicit storage, and structural
generic methods; it does not depend on those libraries.
