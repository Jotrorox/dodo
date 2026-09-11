"""Exercise release preflight with real archives and a stub Cargo invocation."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


BUILD_SCRIPT = Path(__file__).with_name("build-release.sh")


@unittest.skipUnless(shutil.which("cc") and shutil.which("ar"), "requires cc and ar")
class ReleasePrerequisiteTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dodo-release-preflight-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.scripts = self.directory / "scripts"
        self.scripts.mkdir()
        shutil.copyfile(BUILD_SCRIPT, self.scripts / BUILD_SCRIPT.name)
        self.bin = self.directory / "bin"
        self.bin.mkdir()
        self.libraries = self.directory / "static libraries"
        self.libraries.mkdir()
        self.object = self.directory / "fixture.o"
        subprocess.run(
            ["cc", "-x", "c", "-c", "-o", str(self.object), "-"],
            input="void dodo_archive_fixture(void) {}", text=True, check=True,
            capture_output=True,
        )
        for library in ["z", "zstd", "stdc++", "ffi", "c", "m", "z3"]:
            self.archive(self.libraries / f"lib{library}.a")
        self.executable("rustc", "#!/bin/sh\necho 'host: x86_64-unknown-linux-gnu'\n")
        self.executable(
            "llvm-config",
            "#!/bin/sh\nprintf '%s\\n' \"$DODO_TEST_SYSTEM_LIBRARIES\"\n",
        )
        self.executable(
            "cargo",
            "#!/usr/bin/env python3\nimport json, os\n"
            "from pathlib import Path\n"
            "Path(os.environ['DODO_TEST_FLAGS']).write_text(\n"
            "    json.dumps(os.environ['CARGO_ENCODED_RUSTFLAGS'].split('\\x1f')))\n"
            "raise SystemExit(41)\n",
        )
        self.flags_file = self.directory / "flags.json"
        self.environment = {
            **os.environ,
            "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
            "CC": shutil.which("cc"),
            "LIBRARY_PATH": str(self.libraries),
            "LLVM_SYS_221_PREFIX": str(self.directory),
            "DODO_TEST_FLAGS": str(self.flags_file),
        }
        self.environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
        self.environment.pop("RUSTFLAGS", None)

    def executable(self, name, content):
        path = self.bin / name
        path.write_text(content)
        path.chmod(0o755)

    def archive(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["ar", "rcs", str(path), str(self.object)], check=True, capture_output=True,
        )

    def run_preflight(self, system_libraries):
        return subprocess.run(
            ["bash", str(self.scripts / BUILD_SCRIPT.name), "release-small-static"],
            env={**self.environment, "DODO_TEST_SYSTEM_LIBRARIES": system_libraries},
            text=True, capture_output=True,
        )

    def assert_cargo_reached(self, result, archive_directory):
        self.assertEqual(result.returncode, 41, result.stderr)
        self.assertIn(f"native={archive_directory}", json.loads(self.flags_file.read_text()))

    def test_named_library_resolves_from_library_path(self):
        self.assert_cargo_reached(self.run_preflight("-lz3"), self.libraries)

    def test_absolute_shared_library_uses_static_search_path(self):
        for filename in ["libz3.so", "libz3.so.4.8"]:
            with self.subTest(filename=filename):
                result = self.run_preflight(str(self.directory / "shared" / filename))
                self.assert_cargo_reached(result, self.libraries)

    def test_absolute_archive_adds_its_directory(self):
        archive = self.directory / "llvm-libs" / "libextra.a"
        self.archive(archive)
        self.assert_cargo_reached(self.run_preflight(str(archive)), archive.parent)

    def test_shared_library_prefers_sibling_archive(self):
        archive = self.directory / "llvm-libs" / "libextra.a"
        self.archive(archive)
        result = self.run_preflight(str(archive.with_suffix(".so")))
        self.assert_cargo_reached(result, archive.parent)

    def test_missing_static_archive_fails_before_cargo(self):
        result = self.run_preflight(str(self.directory / "libdodo_missing_fixture.so"))
        self.assertEqual(result.returncode, 1)
        self.assertIn("Missing static archive libdodo_missing_fixture.a", result.stderr)
        self.assertFalse(self.flags_file.exists())

    def test_unsupported_linker_flag_fails_before_cargo(self):
        result = self.run_preflight("-Wl,--as-needed")
        self.assertEqual(result.returncode, 1)
        self.assertIn("Unsupported LLVM system-library flag", result.stderr)
        self.assertFalse(self.flags_file.exists())


if __name__ == "__main__":
    unittest.main()
