"""Exercise the linkage check against ELF files produced by the system linker."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


CHECKER = Path(__file__).with_name("check-linkage.py")


@unittest.skipUnless(shutil.which("cc") and shutil.which("readelf"), "requires cc and readelf")
class LinkageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="dodo-linkage-")
        cls.directory = Path(cls.temporary.name)
        cls.compile("static", "void _start(void) {}", "-nostdlib", "-static")
        cls.compile("dynamic", "int main(void) { return 0; }")
        cls.compile(
            "custom-interpreter", "int main(void) { return 0; }",
            "-Wl,--dynamic-linker,/opt/toolchain/ld-linux-x86-64.so.2",
        )
        cls.compile("interpreter-only", "void _start(void) {}", "-nostdlib", "-pie")
        cls.compile("libLLVM-fixture.so", "void llvm_fixture(void) {}", "-shared", "-fPIC")
        cls.compile(
            "llvm", "void llvm_fixture(void); int main(void) { llvm_fixture(); return 0; }",
            "-L" + str(cls.directory), "-lLLVM-fixture",
        )
        cls.compile("libsupport-fixture.so", "void support_fixture(void) {}", "-shared", "-fPIC")
        cls.compile(
            "support", "void support_fixture(void); int main(void) { support_fixture(); return 0; }",
            "-L" + str(cls.directory), "-lsupport-fixture",
        )
        cls.compile("object", "int value;", "-c")

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    @classmethod
    def compile(cls, name, source, *flags):
        subprocess.run(
            ["cc", "-x", "c", "-", *flags, "-o", str(cls.directory / name)],
            input=source, text=True, capture_output=True, check=True,
        )

    def check(self, name, *flags, env=None):
        return subprocess.run(
            [sys.executable, str(CHECKER), str(self.directory / name), *flags],
            capture_output=True, text=True, env=env,
        )

    def test_static_binary_passes_all_modes(self):
        for flags in [(), ("--static",), ("--release",)]:
            with self.subTest(flags=flags):
                result = self.check("static", *flags)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_default_allows_system_libraries(self):
        result = self.check("dynamic")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("libc.so", result.stdout)

    def test_default_rejects_shared_llvm(self):
        result = self.check("llvm")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("libLLVM-fixture.so", result.stderr)

    def test_static_rejects_system_libraries(self):
        result = self.check("dynamic", "--static")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("libc.so", result.stderr)

    def test_release_allows_standard_system_libraries(self):
        result = self.check("dynamic", "--release")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_release_rejects_llvm_and_extra_libraries(self):
        for name, library in [("llvm", "libLLVM-fixture.so"), ("support", "libsupport-fixture.so")]:
            with self.subTest(name=name):
                result = self.check(name, "--release")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(library, result.stderr)

    def test_release_rejects_nonstandard_interpreter(self):
        result = self.check("custom-interpreter", "--release")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("/opt/toolchain/ld-linux-x86-64.so.2", result.stderr)

    def test_static_rejects_interpreter_without_needed_libraries(self):
        result = self.check("interpreter-only", "--static")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("PT_INTERP", result.stderr)
        self.assertNotIn("shared libraries:", result.stderr)

    def test_invalid_files_fail(self):
        (self.directory / "text").write_text("not an ELF file")
        (self.directory / "truncated").write_bytes((self.directory / "static").read_bytes()[:64])
        for name in ["missing", "text", "truncated", "object"]:
            with self.subTest(name=name):
                result = self.check(name)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("linkage check failed", result.stderr)

    def test_missing_readelf_fails(self):
        result = self.check("static", env={**os.environ, "PATH": ""})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("readelf", result.stderr)


if __name__ == "__main__":
    unittest.main()
