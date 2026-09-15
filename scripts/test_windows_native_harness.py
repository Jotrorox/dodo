#!/usr/bin/env python3
"""Check native fixture failure reporting and Windows timeout cleanup."""

import ctypes
import os
from pathlib import Path
import sys
import tempfile
import unittest

from test_windows_native import run


class NativeRunnerTests(unittest.TestCase):
    def test_arguments_and_working_directory(self):
        with tempfile.TemporaryDirectory(prefix="native runner space ") as directory:
            work = Path(directory)
            script = work / "child script.py"
            script.write_text("import pathlib, sys\n"
                              "pathlib.Path('result').write_text('|'.join(sys.argv[1:]), encoding='utf-8')\n")
            run([sys.executable, str(script), "space arg", "é", ""], cwd=work, env=os.environ)
            self.assertEqual((work / "result").read_text(encoding="utf-8"), "space arg|é|")

    def test_nonzero_status_preserves_both_output_streams(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(RuntimeError) as failure:
                run([sys.executable, "-c",
                     "import os; os.write(1, b'output\\xff\\n'); os.write(2, b'failure\\n'); raise SystemExit(73)"],
                    cwd=directory, env=os.environ)
            self.assertIn("Failed (73)", str(failure.exception))
            self.assertIn("output\ufffd", str(failure.exception))
            self.assertIn("failure", str(failure.exception))

    @unittest.skipUnless(os.name == "nt", "requires Windows taskkill and process handles")
    def test_timeout_terminates_descendants(self):
        with tempfile.TemporaryDirectory(prefix="native timeout ") as directory:
            work = Path(directory)
            with self.assertRaisesRegex(RuntimeError, "Timed out after 5s"):
                run([sys.executable, "-c",
                     "import pathlib, subprocess, sys, time; "
                     "child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)']); "
                     "pathlib.Path('child-pid').write_text(str(child.pid)); "
                     "time.sleep(60)"], cwd=work, env=os.environ, timeout=5)
            pid = int((work / "child-pid").read_text())
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
            kernel32.OpenProcess.restype = ctypes.c_void_p
            kernel32.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
            kernel32.WaitForSingleObject.restype = ctypes.c_uint32
            kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
            handle = kernel32.OpenProcess(0x100000, False, pid)  # SYNCHRONIZE
            if handle:
                try:
                    self.assertEqual(kernel32.WaitForSingleObject(handle, 0), 0,
                                     "timed-out fixture left a running child")
                finally:
                    kernel32.CloseHandle(handle)
            else:
                self.assertEqual(ctypes.get_last_error(), 87)  # PID no longer exists


if __name__ == "__main__":
    unittest.main()
