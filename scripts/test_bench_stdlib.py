#!/usr/bin/env python3
"""Routing benchmark correctness guards, without timing thresholds."""
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import bench_stdlib as bench


class ChecksumValidation(unittest.TestCase):
    def test_mixed_head_falls_back_to_get_and_counts_partial_cycles(self):
        _, expected, metadata = bench.dynamic_fixture(7, "mixed")
        self.assertEqual(expected["hit"], [1, 2, 3, 4, 5, 6, 7])
        self.assertEqual(expected["head"], [1, 1, 3, 4, 5, 5, 7])
        self.assertEqual(bench.sequence_checksum(17, expected["head"]), 57)
        self.assertEqual(metadata["largest_prefix_group"], 7)
        self.assertEqual(metadata["prefix_groups"], 1)

    def test_bad_checksum_and_malformed_output_are_rejected(self):
        for output in (b"100 99", b"100", b"0 6"):
            with self.subTest(output=output), patch.object(bench, "run", return_value=output):
                with self.assertRaises(RuntimeError):
                    bench.measure(Path("unused"), 0, b"", 3, lambda n: n * 2)


class NativeRouterSmoke(unittest.TestCase):
    def test_all_shapes_requests_and_optimization_levels(self):
        compiler = Path(os.environ.get("BENCH_STDLIB_BINARY", bench.ROOT / "target/debug/dodo")).resolve()
        if not compiler.is_file():
            self.skipTest("build Dodo or set BENCH_STDLIB_BINARY")
        linker = shutil.which(os.environ.get("DODO_CC", "cc"))
        with tempfile.TemporaryDirectory(prefix="dodo-routing-smoke-") as directory:
            for optimization in (0, 3):
                for shape in bench.ROUTE_SHAPES:
                    with self.subTest(optimization=optimization, shape=shape):
                        # Seven exercises incomplete mixed groups and wraparound.
                        source, checksums, _ = bench.dynamic_fixture(7, shape)
                        binary, _ = bench.compile_fixture(
                            str(compiler), linker, Path(directory), f"router_{shape}", optimization, source,
                            (bench.FIXTURES / "web_dynamic_runtime.c", bench.ROOT / "stdlib/std/time/runtime.c"))
                        for request_mode, request in enumerate(bench.ROUTE_REQUESTS):
                            for index_mode in (0, 1):
                                bench.measure(binary, request_mode * 2 + index_mode, b"", 17,
                                              lambda n, values=checksums[request]: bench.sequence_checksum(n, values))


if __name__ == "__main__":
    unittest.main()
