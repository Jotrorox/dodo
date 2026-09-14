#!/usr/bin/env python3
"""Exercise the Wine harness entry wrapper without requiring Windows tools."""
import platform
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_stdlib_windows import runtime_source


@unittest.skipUnless(platform.system() == "Linux" and platform.machine() == "x86_64"
                     and shutil.which("cc"), "requires Linux x64 and a C compiler")
class WindowsStartupTests(unittest.TestCase):
    def test_void_ignores_register_contents_and_i32_preserves_exit_status(self):
        # Execute the exact generated wrapper with a local ExitProcess shim.
        # A void callee may leave anything in EAX, even after successful work.
        # Deliberately leave 73 there so reading it as an int cannot pass by luck.
        for result_type in ("", " -> void", " -> i32"):
            for optimization in ("0", "3"):
                with self.subTest(result_type=result_type, optimization=optimization):
                    runtime = runtime_source(f"package fixture\nfn main(){result_type} {{}}\n")
                    callee = ('int dodo_main(void) { return 73; }' if result_type == " -> i32" else
                              'void dodo_main(void) { __asm__ volatile("movl $73, %%eax" ::: "eax"); }')
                    source = ("#define __declspec(x)\n#define __stdcall\n" + runtime +
                              '\n#include <stdlib.h>\n'
                              'void ExitProcess(unsigned int code) { _Exit(code); }\n'
                              '__attribute__((noinline)) ' + callee +
                              '\nint main(void) { mainCRTStartup(); return 99; }\n')
                    with tempfile.TemporaryDirectory(prefix="dodo-startup-") as directory:
                        path = Path(directory)
                        (path / "startup.c").write_text(source)
                        subprocess.run(["cc", "-std=c11", "-Wall", "-Wextra", "-Werror",
                                        "-fno-builtin", "-O" + optimization,
                                        str(path / "startup.c"), "-o", str(path / "startup")], check=True)
                        result = subprocess.run([str(path / "startup")], timeout=10)
                        self.assertEqual(result.returncode, 73 if result_type == " -> i32" else 0)


if __name__ == "__main__":
    unittest.main()
