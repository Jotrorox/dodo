#!/usr/bin/env python3
"""Regression tests for the OpenGL binding generator.

Run with `python3 scripts/test_generate_opengl.py`. A small inline registry
stands in for gl.xml, so no download or compiler is needed.
"""

from pathlib import Path
import tempfile
import unittest

import generate_opengl as generator

REGISTRY = """<?xml version="1.0" encoding="UTF-8"?>
<registry>
    <types/>
    <enums namespace="GL" group="Example">
        <enum value="0x00004000" name="GL_COLOR_BUFFER_BIT"/>
        <enum value="0x0007" name="GL_QUADS"/>
        <enum value="0xFFFFFFFFFFFFFFFF" type="ull" name="GL_TIMEOUT_IGNORED"/>
        <enum value="0x8B31" name="GL_VERTEX_SHADER"/>
        <enum value="0x1" api="gles2" name="GL_ONLY_ES"/>
    </enums>
    <commands namespace="GL">
        <command>
            <proto>void <name>glClear</name></proto>
            <param><ptype>GLbitfield</ptype> <name>mask</name></param>
        </command>
        <command>
            <proto>void <name>glBegin</name></proto>
            <param><ptype>GLenum</ptype> <name>mode</name></param>
        </command>
        <command>
            <proto>const <ptype>GLubyte</ptype> *<name>glGetString</name></proto>
            <param><ptype>GLenum</ptype> <name>name</name></param>
        </command>
        <command>
            <proto>void <name>glShaderSource</name></proto>
            <param><ptype>GLuint</ptype> <name>shader</name></param>
            <param><ptype>GLsizei</ptype> <name>count</name></param>
            <param>const <ptype>GLchar</ptype> *const*<name>string</name></param>
            <param>const <ptype>GLint</ptype> *<name>length</name></param>
        </command>
        <command>
            <proto><ptype>GLsync</ptype> <name>glFenceSync</name></proto>
            <param><ptype>GLenum</ptype> <name>condition</name></param>
            <param><ptype>GLbitfield</ptype> <name>flags</name></param>
        </command>
        <command>
            <proto>void <name>glDebugMessageCallback</name></proto>
            <param><ptype>GLDEBUGPROC</ptype> <name>callback</name></param>
            <param>const void *<name>userParam</name></param>
        </command>
        <command>
            <proto>void *<name>glMapBuffer</name></proto>
            <param><ptype>GLenum</ptype> <name>target</name></param>
            <param><ptype>GLenum</ptype> <name>type</name></param>
        </command>
    </commands>
    <feature api="gl" name="GL_VERSION_1_0" number="1.0">
        <require>
            <command name="glClear"/>
            <command name="glBegin"/>
            <command name="glGetString"/>
            <enum name="GL_COLOR_BUFFER_BIT"/>
            <enum name="GL_QUADS"/>
        </require>
    </feature>
    <feature api="gl" name="GL_VERSION_2_0" number="2.0">
        <require>
            <command name="glShaderSource"/>
            <command name="glMapBuffer"/>
            <enum name="GL_VERTEX_SHADER"/>
        </require>
    </feature>
    <feature api="gl" name="GL_VERSION_3_2" number="3.2">
        <require>
            <command name="glFenceSync"/>
            <enum name="GL_TIMEOUT_IGNORED"/>
        </require>
        <remove profile="core">
            <command name="glBegin"/>
            <enum name="GL_QUADS"/>
        </remove>
    </feature>
    <feature api="gl" name="GL_VERSION_4_3" number="4.3">
        <require>
            <command name="glDebugMessageCallback"/>
        </require>
    </feature>
    <feature api="gl" name="GL_VERSION_4_7" number="4.7">
        <require>
            <command name="glFuture"/>
        </require>
    </feature>
    <feature api="gles2" name="GL_ES_VERSION_2_0" number="2.0">
        <require>
            <enum name="GL_ONLY_ES"/>
        </require>
    </feature>
</registry>
"""


class NamingTests(unittest.TestCase):
    def test_commands_become_snake_case(self):
        cases = {
            "ClearColor": "clear_color",
            "TexImage2D": "tex_image_2d",
            "TexImage2DMultisample": "tex_image_2d_multisample",
            "Uniform4fv": "uniform_4fv",
            "UniformMatrix2x3fv": "uniform_matrix_2x3fv",
            "VertexAttribI4ui": "vertex_attrib_i4ui",
            "VertexAttribIPointer": "vertex_attrib_i_pointer",
            "VertexAttrib4Nub": "vertex_attrib_4nub",
            "GetInteger64i_v": "get_integer_64i_v",
            "DispatchCompute": "dispatch_compute",
        }
        for name, expected in cases.items():
            self.assertEqual(generator.snake(name), expected, name)

    def test_reserved_words_get_an_underscore(self):
        self.assertEqual(generator.identifier("type"), "type_")
        self.assertEqual(generator.identifier("from"), "from_")
        self.assertEqual(generator.identifier("bufSize"), "buf_size")


class TypeTests(unittest.TestCase):
    def test_c_types_map_to_dodo(self):
        cases = {
            "GLenum": "u32",
            "GLboolean": "u8",
            "GLsizeiptr": "isize",
            "void": "void",
            "void *": "*mut u8",
            "const void *": "*const u8",
            "const GLchar *const*": "*const *const u8",
            "GLchar **": "*mut *mut u8",
            "void **": "*mut *mut u8",
            "GLsync": "*mut u8",
            "GLDEBUGPROC": generator.DEBUG_PROC,
        }
        for text, expected in cases.items():
            self.assertEqual(generator.dodo_type(text, text), expected, text)

    def test_unknown_types_fail(self):
        with self.assertRaises(SystemExit):
            generator.dodo_type("struct _cl_context *", "context")


class RenderTests(unittest.TestCase):
    def render(self) -> str:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gl.xml"
            path.write_text(REGISTRY)
            return generator.render(path)

    def test_core_profile_up_to_4_6(self):
        text = self.render()
        self.assertIn("pub const COMMAND_COUNT: usize = 6", text)
        self.assertIn('const NAMES: &[u8] = b"glClear\\0glGetString\\0glShaderSource\\0glMapBuffer\\0glFenceSync\\0glDebugMessageCallback\\0"', text)
        self.assertNotIn("begin", text)
        self.assertNotIn("QUADS", text)
        self.assertNotIn("glFuture", text)
        self.assertNotIn("ONLY_ES", text)

    def test_declarations(self):
        text = self.render()
        self.assertIn("pub const COLOR_BUFFER_BIT: u32 = 0x00004000\n", text)
        self.assertIn("pub const TIMEOUT_IGNORED: u64 = 0xffffffffffffffff\n", text)
        self.assertIn("pub unsafe fn get_string(name: u32) -> *const u8 {\n", text)
        self.assertIn("pub unsafe fn map_buffer(target: u32, type_: u32) -> *mut u8 {\n", text)
        self.assertIn("pub unsafe fn fence_sync(condition: u32, flags: u32) -> *mut u8 {\n", text)
        self.assertIn(
            "pub unsafe fn shader_source(shader: u32, count: i32, string: *const *const u8, length: *const i32) {\n"
            "    unsafe {\n"
            "        address := ADDRESSES[2]\n"
            "        core.assert(address != 0) // glShaderSource is not loaded\n"
            '        call := address as extern "C" fn(u32, i32, *const *const u8, *const i32)\n'
            "        call(shader, count, string, length)\n"
            "    }\n"
            "}\n",
            text,
        )
        self.assertIn("        return call(name)\n", text)


if __name__ == "__main__":
    unittest.main()
