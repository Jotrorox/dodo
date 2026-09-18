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

JSON has six value kinds: null, boolean, number, string, array, and object. An
object associates string names with values; an array stores values in order.
Choose a typed struct when you know the expected fields, or a borrowed `Value`
when you need to inspect a document whose shape varies.

| Goal | Starting point |
| --- | --- |
| Decode a known object shape | `@derive(Json)` and `json.decode::<T>(bytes)` |
| Inspect a document or primitive root | `json.parse(bytes)` and `Value` accessors |
| Parse wide objects with a key index | `json.parse_indexed(bytes, &mut scratch)` |
| Accept escaped external strings | A `json.String` field and its `decode` method |
| Encode your own struct | `json.to_slice(&value, &mut storage)` |
| Generate values one token at a time | `json.Encoder.new(&mut writer)` |
| Read a complete bounded document from I/O | `json.parse_reader` or `decode_reader` |
| Read several whitespace-separated documents | `json.Decoder` over existing bytes |

Decoding generally returns views into the input rather than copying its text.
Keep the input alive until all decoded views are finished. Encoding needs a
destination whose capacity you choose. [Byte I/O](io.md), [UTF-8 text](text.md),
and [Results](patterns-and-results.md) provide the underlying concepts.

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

The steps are: declare the expected fields, decode and validate a complete
document, use ordinary typed fields, then encode into a separate destination.
The output matches this particular input because its field order and compact
representation match the derived encoder. In general, decode/encode preserves
the JSON value, not original whitespace or escape spellings.

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
Derived array decoders advance through elements sequentially, with no repeated
index lookup or separate length pass. Primitive arrays use a generated loop.
Derived struct decoders traverse the object once, dispatching decoded field names
through a generated hash decision tree and tracking which members were present.
They retain field views in storage proportional to the schema size, then convert
them in declaration order. Unknown-field checks precede conversion, and borrowed
fields keep their dependency on the input. Hash collisions receive full name
comparisons. Document validation, including duplicate detection, is separate.
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
| Inspect JSON using a scratch key index | `json.parse_indexed(input, &mut scratch) -> json.Value!json.Error` |
| Inspect text using a scratch key index | `json.parse_str_indexed(input, &mut scratch) -> json.Value!json.Error` |
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

### Indexed parsing

For wide objects, `parse_indexed` and `parse_str_indexed` use caller-provided
`&mut[usize]` scratch storage to avoid repeatedly scanning earlier members.
They enforce the same JSON syntax, decoded-name uniqueness, and nesting limit
as `parse`. Hash collisions are resolved by comparing complete decoded names.

```dodo test
package indexed_json
import "std/encoding/json"

fn main() {
    scratch := [0usize; 128 * json.KEY_INDEX_WORDS]
    value := json.parse_indexed(b"{\"name\":\"Ada\",\"age\":36}", &mut scratch)!
    // Scratch can be reused while earlier input-backed values remain live.
    next := json.parse_str_indexed("{\"ok\":true}", &mut scratch)!
    age := value.require("age")!
    ok := next.require("ok")!
    assert_eq(age.as_u64()!, 36u64)
    assert(ok.as_bool()!)
}
```

Each live object member requires `json.KEY_INDEX_WORDS` (five) words. Capacity
is `scratch.len / json.KEY_INDEX_WORDS`; any remaining words are unused. Entries
belong to open objects and are released when those objects close, so nested
objects share the same storage and successive objects in an array reuse it.
Size scratch for the largest combined number of members already encountered in
all currently open objects, including the member whose value is being parsed.
A flat 1,024-field object needs 5,120 words (40 KiB on a 64-bit target).

Insufficient capacity returns `BufferFull` at the new key's opening quote; it
does not silently fall back to rescanning. Empty scratch accepts documents
without object members. Each call resets the index, including after a failed
parse, and returned values borrow only the input. Use `json.from_value::<T>`
to decode an indexed result with an existing derived or custom codec.

Index setup takes time proportional to scratch capacity. With well-distributed
hashes, validation takes expected linear time in input size plus that setup;
deliberately colliding keys can still cause quadratic comparisons. The original
`parse` entry points keep their constant-extra-memory duplicate detection.

## Borrowed values and strings

### Input ownership

`Value` and decoded `json.String` retain checked shared borrows of their input.
Keep the input alive and unchanged while using them. A decoded struct containing
either view has the same lifetime dependency. Decoding an object does not build
an owned heap tree; array/object navigation scans the validated source. Repeated
lookups can therefore repeat work. The `parse` and `parse_str` entry points use
constant extra storage for duplicate-name validation and compare earlier
members; their work grows quadratically with the number of object members.
Use indexed parsing to supply scratch storage for this check. Apply application
input limits to large external documents.

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

### Decode an escaped string into your own storage

The input below represents a JSON string whose final letter is written as a
Unicode escape. Its decoded value is `café`. Four Unicode scalars need five
UTF-8 bytes, so capacity should be based on `decoded_len`, not character count.

```dodo test
package json_string_bytes
import "std/encoding/json"

fn example() -> void!json.Error {
    root := json.parse(b"\"caf\\u00e9\"")?
    value := root.as_string()?
    assert(value.is_escaped())
    assert(value.equals("café"))
    assert_eq(value.decoded_len(), 5usize)
    storage := [0u8; 16]
    decoded := value.decode(&mut storage)?
    assert_eq(decoded, "café")
    return ok()
}

fn main() -> i32 {
    match example() {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

Save as `json_string_bytes.dodo` and run `dodo run json_string_bytes.dodo`.
It exits with zero and prints nothing. The final `decoded` view borrows
`storage`; it does not depend on the original JSON bytes. Calling `as_str()`
on `value` instead would return `EscapedString` because the representation
contains an escape sequence.

## Inspecting JSON without a struct

`Value.kind()` identifies the JSON kind. Fallible conversions `as_bool()`,
`as_i64()`, `as_u64()`, `as_f64()`, `as_string()`, and `as_str()` check it;
`is_null()` recognizes null explicitly. `get(name)` looks up an optional object
member, while `require(name)` reports a missing field. `at(index)` selects an
array element. `len()` reports an array or object's member count. Array and
object iterators expose members without allocating a collection.

`get(name)` returns `Option<Value>` and returns `none` for a missing member or
a non-object. `at(index)` likewise returns `none` for a missing index or a
non-array. Use `require(name)`, `require_object()`, `array()`, or `object()`
when you need a type error rather than an absent optional value.

### Iterate an array

`array()` checks the root kind once and creates a cursor. `next()` returns a
borrowed child or `none` at the end; its input is already validated, so advancing
the iterator does not return a parsing `Result`. Each scalar conversion still
checks the child's type and range.

```dodo test
package json_array_values
import "std/encoding/json"

fn example() -> void!json.Error {
    root := json.parse(b"[10,20,30]")?
    values := root.array()?
    total := 0i64
    for {
        match values.next() {
            some(value) => { total += value.as_i64()? },
            none => { break },
        }
    }
    assert_eq(total, 60i64)
    return ok()
}

fn main() -> i32 {
    match example() {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

Save as `json_array_values.dodo` and run `dodo run json_array_values.dodo`.
Success exits with zero. `object()?.next()` uses the same pattern and returns
`Entry { key: json.String, value: json.Value }`. Consume or finish using the
current child before advancing either iterator again.

### Navigate with a pointer or move a borrowed view

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

| Error kind | Typical cause | Response |
| --- | --- | --- |
| `Syntax`, `TrailingData`, `Depth` | Malformed input, extra data, or excessive nesting | Reject the document and inspect the byte position. |
| `TypeMismatch`, `NumberRange` | A value does not fit the requested Dodo field type | Correct the schema or reject that input. |
| `MissingField`, `DuplicateField`, `UnknownField` | An object violates the required field rules | Correct the document or choose the intended unknown-field policy. |
| `BufferFull` | Caller storage cannot hold the input, decoded text, or output | Increase the relevant bound or reject the oversized value. |
| `EscapedString` | Borrowing `&str` would require unescaping | Use `json.String.decode` into caller-owned storage. |
| `Io` | The underlying reader or writer failed | Treat an emitted prefix as incomplete and inspect `Error` metadata. |

## Custom codecs and token-by-token encoding

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

`Encoder.pretty(&mut writer)` selects indentation; `Encoder.new` selects compact
output. `written()` is the emitted byte count so far. Both maintain the same
nesting, token-order, and error checks. They borrow the destination exclusively,
so end the encoder's scope before inspecting the destination directly.

The encoder is useful when producing a simple object or a primitive root without
declaring a struct. It writes punctuation and escapes strings for you. A key
must be followed by exactly one value, and every opened container must be closed.

```dodo test
package json_tokens
import "std/encoding/json"
import "std/io"
import "core/bytes"

fn example() -> void!json.Error {
    storage := [0u8; 64]
    writer := io.MemoryWriter.new(&mut storage)
    {
        encoder := json.Encoder.new(&mut writer)
        encoder.begin_object()?
        encoder.key("ok")?
        encoder.boolean(true)?
        encoder.key("n")?
        encoder.unsigned(3)?
        encoder.end_object()?
        core.drop(encoder.finish()?)
    }
    assert(bytes.equal(writer.written(), b"{\"ok\":true,\"n\":3}"))
    return ok()
}

fn main() -> i32 {
    match example() {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

Save as `json_tokens.dodo` and run `dodo run json_tokens.dodo`. The assertion
checks the complete compact output. To write a primitive root, use just the
appropriate value method and `finish()`, without `begin_object` or `key`.

For custom decoding that must return input-borrowing fields, `take_field(value,
name)` and `take_element(value, index)` consume their parent view and return
`Field { rest, value }`. Both outputs retain the original input borrow; `rest`
still views the complete parent, so it can be used to extract another field.
`take_optional_field` returns `OptionalField { rest, value: Option<Value> }`.
These helpers do not delete members or modify input; they transfer checked
ownership dependencies. `check_fields(keys, deny_unknown)` requires an object
and rejects unlisted names when `deny_unknown` is true; required-field checks
still happen when extracting each field. Prefer derivation until you need a custom
representation or validation rule.

For sequential extraction, `ArrayCursor.new(value)` consumes an array view.
Each consuming `take()` returns `ArrayElement { rest, value }`: decode the child
and continue with `rest`. Children retain the original input borrow and can
remain live while the cursor advances. `take()` reports `TypeMismatch` at the
array's position if there are no elements left; `finish()` reports the same
error if any remain. Taking the expected number of elements and then calling
`finish()` checks a fixed array's length without counting it first. Conversion
errors are reported as elements are decoded, so an invalid element can be
reported before a length mismatch later in the array.

The [API reference](stdlib-api.md) includes every encoder, decoder, projection,
iterator, and error declaration.

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
