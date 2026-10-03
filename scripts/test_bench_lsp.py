#!/usr/bin/env python3
"""Correctness guards for the LSP timing harness (no latency assertions)."""
import copy
import pathlib
import unittest

import bench_lsp


class DiagnosticValidation(unittest.TestCase):
    def setUp(self):
        self.uri = "file:///main.dodo"
        self.expected = {self.uri: [(2, "unknown binding `missing`")]}
        self.versions = {self.uri: 7}
        self.publications = {self.uri: {
            "version": 7,
            "diagnostics": [{"severity": 1, "message": "unknown binding `missing`",
                             "range": {"start": {"line": 2, "character": 20}}}],
        }}

    def validate(self):
        bench_lsp.validate_diagnostics(self.publications, self.expected, self.versions)

    def test_intended_error_is_allowed(self):
        self.validate()

    def test_missing_publication_and_stale_version_are_rejected(self):
        publication = self.publications.pop(self.uri)
        with self.assertRaises(AssertionError):
            self.validate()
        self.publications[self.uri] = publication
        publication["version"] = 6
        with self.assertRaises(AssertionError):
            self.validate()

    def test_extra_dependency_error_is_rejected(self):
        self.publications["file:///dependency.dodo"] = copy.deepcopy(self.publications[self.uri])
        with self.assertRaises(AssertionError):
            self.validate()

    def test_wrong_error_and_missing_error_are_rejected(self):
        self.publications[self.uri]["diagnostics"][0]["message"] = "different failure"
        with self.assertRaises(AssertionError):
            self.validate()
        self.publications[self.uri]["diagnostics"] = []
        with self.assertRaises(AssertionError):
            self.validate()

    def test_null_queries_are_rejected(self):
        for method in ("hover", "completion", "definition", "signatureHelp"):
            with self.subTest(method=method), self.assertRaises(AssertionError):
                bench_lsp.validate_query(method, {"result": None}, self.uri)


class RealServerSmoke(unittest.TestCase):
    def test_all_workloads_and_edit_modes(self):
        # Override through BENCH_LSP_BINARY for other build profiles.
        import os
        binary = pathlib.Path(os.environ.get("BENCH_LSP_BINARY", "target/release/dodo")).resolve()
        if not binary.is_file():
            self.skipTest("build the release compiler or set BENCH_LSP_BINARY")
        for workload in bench_lsp.DEFAULT_SIZES:
            for edit in ("body", "comment"):
                with self.subTest(workload=workload, edit=edit):
                    result = bench_lsp.measure(str(binary), workload, 2, 2, True, 2,
                                               False, 3,
                                               "package" if workload == "project" else "file",
                                               edit, warmup=1)
                    self.assertEqual(result["samples"], 2)
        # Also cover single-file checking inside a manifest workspace.
        bench_lsp.measure(str(binary), "project", 2, 2, True, 2,
                          True, 3, "file", "body", warmup=1)
        bench_lsp.measure(str(binary), "large-function", 2, 2, True, 1,
                          False, 3, "file", "body", warmup=1, body_shape="assignments")
        bench_lsp.measure(str(binary), "project", 2, 2, True, 2,
                          False, 3, "package", "manifest", warmup=1)


if __name__ == "__main__":
    unittest.main()
