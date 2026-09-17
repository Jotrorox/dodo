#!/usr/bin/env python3
"""Regression tests for source extraction and navigable API-page generation.

Run with `python3 -m unittest discover -s scripts -p test_generate_api_docs.py`.
No compiler, third-party packages, or generated pages are required.
"""

from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import generate_api_docs as api


class ExtractionTests(unittest.TestCase):
    def test_compact_fields_are_each_emitted_once(self):
        source = """package example
pub struct Packet {
    // Number of bytes delivered.
    pub count: usize, pub ready: bool; pub code: i32
    hidden: u8
    pub fn len(&self) -> usize { return self.count }
}
"""
        declarations = api.extract(source)
        self.assertEqual([(d.name, d.owner) for d in declarations], [("Packet", ""), ("len", "Packet")])
        self.assertEqual(declarations[0].signature, """pub struct Packet {
    // Number of bytes delivered.
    pub count: usize
    pub ready: bool
    pub code: i32
    // Private implementation fields omitted.
}""")
        self.assertEqual(declarations[1].signature, "pub fn len(&self) -> usize")

    def test_multiline_fields_do_not_split_generic_arguments(self):
        source = """pub struct Store {
    pub values:
        Result<
            [2]u8,
            Problem
        >
    pub [4]u16 historical
}
"""
        declaration, = api.extract(source)
        self.assertIn("Result<\n    [2]u8,\n    Problem\n    >", declaration.signature)
        self.assertIn("pub [4]u16 historical", declaration.signature)
        self.assertNotIn("Private implementation", declaration.signature)

    def test_multiline_return_and_effects_are_complete(self):
        source = """// Preserve the original input dependency.
pub fn fetch<T, E>(
    input: &[T],
    index: usize,
) ->
    Result<
        Option<&T>,
        E
    > from(
        input
    ) requires_plain(E)
{
    return ok(none)
}
pub extern "C" fn foreign_call(
    data: *const u8,
    length: usize,
) -> i32
"""
        fetch, foreign = api.extract(source)
        self.assertTrue(fetch.signature.endswith("> from(\n        input\n    ) requires_plain(E)"))
        self.assertNotIn("return ok", fetch.signature)
        self.assertEqual(fetch.comment, "Preserve the original input dependency.")
        self.assertEqual(foreign.name, "foreign_call")
        self.assertTrue(foreign.signature.endswith(") -> i32"))

    def test_private_types_and_method_bodies_are_not_public_api(self):
        source = """  struct Hidden {
    pub secret: usize
    pub fn internal(&self) -> usize { return self.secret }
}
pub struct Visible {
    [4]u8 hidden
    fn helper(&self) -> u8 { return self.hidden[0] }
    pub fn access(&self) -> u8 { return self.helper() }
}
"""
        declarations = api.extract(source)
        self.assertEqual([d.name for d in declarations], ["Visible", "access"])
        self.assertIn("Private implementation fields omitted", declarations[0].signature)
        self.assertNotIn("helper", declarations[0].signature)

    def test_strings_and_byte_literals_cannot_invent_declarations(self):
        source = r'''// pub fn fake() { unmatched comment brace
pub const TEXT: &str = "pub fn fake() { // still a string }"
pub const BYTE: u8 = b'{'
pub const BYTES: &[u8] = b"pub struct Fake {"
pub fn real() { text := "escaped quote: \" }" }
'''
        declarations = api.extract(source)
        self.assertEqual([d.name for d in declarations], ["TEXT", "BYTE", "BYTES", "real"])
        self.assertTrue(declarations[0].signature.endswith('"pub fn fake() { // still a string }"'))
        masked = api.lexical_mask(source)
        self.assertEqual(len(masked), len(source))
        self.assertEqual([i for i, c in enumerate(masked) if c == "\n"],
                         [i for i, c in enumerate(source) if c == "\n"])

    def test_enum_variants_and_payload_names_remain_complete(self):
        source = """@repr(u8)
pub enum Reply<T> {
    Empty,
    // Tuple and named payloads are both public.
    Pair(T, Option<T>),
    Named(code: u8, value: T)
}
"""
        declaration, = api.extract(source)
        self.assertEqual(declaration.signature, source.rstrip())
        self.assertEqual(declaration.line, 2)

    def test_multiline_attributes_and_field_notes_are_preserved(self):
        source = """// Public wire representation.
@derive(
    Json
)
pub struct Record {
    // UTF-8 display name.
    @json_name("displayName")
    pub name: &str
    @compiler(print)
    pub fn print(&self)
}
"""
        record, method = api.extract(source)
        self.assertEqual(record.comment, "Public wire representation.")
        self.assertTrue(record.signature.startswith("@derive(\n    Json\n)\npub struct Record"))
        self.assertIn('// UTF-8 display name.\n    @json_name("displayName")\n    pub name: &str', record.signature)
        self.assertNotIn("Private implementation", record.signature)
        self.assertTrue(method.signature.startswith("@compiler(print)\npub fn print"))

    def test_notes_do_not_cross_blank_lines_or_take_trailing_comments(self):
        source = """// Notes for a different section.

pub const FIRST: u8 = 1 // This belongs to FIRST.
pub const SECOND: u8 = 2
// This belongs to THIRD.
pub const THIRD: u8 = 3
"""
        first, second, third = api.extract(source)
        self.assertEqual(first.comment, "")
        self.assertEqual(second.comment, "")
        self.assertEqual(third.comment, "This belongs to THIRD.")

    def test_constant_initializers_keep_braces_and_comparisons(self):
        source = """pub const LESS: bool = 1 < 2
pub const VALUES: [3]u8 = [
    1, 2, 3
]
pub const POINT: Point = Point { x: 1, y: 2 }
pub const LAST: u8 = 4
"""
        declarations = api.extract(source)
        self.assertEqual([d.name for d in declarations], ["LESS", "VALUES", "POINT", "LAST"])
        self.assertEqual(declarations[2].signature, "pub const POINT: Point = Point { x: 1, y: 2 }")

    def test_bad_sources_fail_instead_of_silently_losing_api(self):
        for source in ("pub type Alias = i32", "pub struct Broken {", 'pub const BAD = "unfinished'):
            with self.subTest(source=source), self.assertRaises(ValueError):
                api.extract(source)

    def test_imports_are_lexical_and_preserve_aliases(self):
        source = '''package example
// import "fake/one"
import "core/bytes" as raw
import "std/io"
pub const SAMPLE: &str = "import fake/two"
'''
        self.assertEqual(api.extract_imports(source), [("raw", "core/bytes"), ("io", "std/io")])

    def test_current_network_error_has_no_duplicated_fields(self):
        source = (api.STDLIB / "std/net.dodo").read_text(encoding="utf-8")
        error = next(d for d in api.extract(source) if d.name == "Error" and d.kind == "struct")
        for field in ("kind", "code", "transferred"):
            self.assertEqual(error.signature.count(f"pub {field}:"), 1)


class GenerationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.stdlib = self.root / "stdlib"
        self.stdlib.mkdir()
        self.output = self.root / "docs/src/content/docs/api"
        self.patches = patch.multiple(api, ROOT=self.root, STDLIB=self.stdlib, OUTPUT=self.output)
        self.patches.start()
        self.addCleanup(self.patches.stop)
        self.addCleanup(self.temporary.cleanup)

    def source(self, name, text):
        path = self.stdlib / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def test_directory_packages_and_neighboring_file_modules(self):
        combined = self.source("std/encoding/json/value.dodo", "  package json\npub struct Value {}\n")
        self.assertEqual(api.module_path(combined), "std/encoding/json")
        child = self.source("std/time/clock.dodo", "package clock\npub fn tick() {}\n")
        self.source("std/time.dodo", "package time\npub struct Time {}\n")
        self.assertEqual(api.module_path(child), "std/time/clock")

    def test_import_links_cover_aliases_intrinsics_and_virtual_providers(self):
        value = self.source("std/encoding/json/value.dodo", '''package json
import "std/io"
import "core/mem"
pub struct Value {}
''')
        decoder = self.source("std/encoding/json/api.dodo", '''package json
import "std/io" as sink
import "std/fs/native" as adapter
pub fn decode() -> Value { return Value {} }
''')
        page, count = api.render_module("std/encoding/json", [value, decoder], 1, {"std/io"})
        self.assertEqual(count, 2)
        self.assertIn("| `sink` | [`std/io`](../io.md)", page)
        self.assertIn("../../../stdlib-api.md#compiler-intrinsics", page)
        self.assertIn("../../../stdlib-api.md#virtual-imports-and-target-providers", page)
        self.assertIn("other source files in the same package", page)
        self.assertIn("Function · [Source]", page)
        self.assertEqual(page, api.render_module("std/encoding/json", [decoder, value], 1, {"std/io"})[0])

    def test_unknown_imports_fail_instead_of_creating_broken_links(self):
        source = self.source("std/example.dodo", 'package example\nimport "std/missing"\npub fn run() {}\n')
        with self.assertRaisesRegex(ValueError, "no API page"):
            api.render_module("std/example", [source], 1, {"std/example"})

    def test_source_notes_and_fences_render_literal_text_safely(self):
        source = self.source("std/example.dodo", '''package example
// Compare <T> and `&T` without generating an HTML tag.
pub const CODE: &str = "```unsafe fn"
pub unsafe fn access() {}
''')
        page, count = api.render_module("std/example", [source], 1, {"std/example"})
        self.assertEqual(count, 2)
        self.assertIn("Compare &lt;T&gt;", page)
        self.assertIn("and `&T`", page)
        self.assertNotIn("`&amp;T`", page)
        self.assertIn('````dodo\npub const CODE: &str = "```unsafe fn"\n````', page)
        self.assertEqual(page.count("Requires an `unsafe` context"), 1)

    def test_generation_is_deterministic_and_counts_declarations_once(self):
        self.source("std/zeta.dodo", "package zeta\npub fn last() {}\n")
        self.source("core/alpha.dodo", "package alpha\npub struct First { pub a: u8, pub b: u8 }\n")
        first = api.generate()
        self.assertEqual(first, api.generate())
        files, modules, declarations = first
        self.assertEqual((modules, declarations), (2, 2))
        self.assertEqual(len(files), 3)
        self.assertIn("**2 source packages**", files[self.output / "index.md"])


if __name__ == "__main__":
    unittest.main()
