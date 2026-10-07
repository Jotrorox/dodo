"""Exercise release tag validation against a temporary repository layout."""

from pathlib import Path
import importlib.util
import json
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("check_release", Path(__file__).with_name("check_release.py"))
check_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_release)

CHANGELOG = """# Changelog

## Unreleased

- Upcoming change.

## 1.2.3 — 2026-10-01

Summary paragraph.

- First change.

## 1.2.2 — 2026-09-01

- Older change.
"""


class CheckReleaseTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="dodo-check-release-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.write_versions("1.2.3", "1.2.3")
        (self.root / "CHANGELOG.md").write_text(CHANGELOG)

    def write_versions(self, cargo, extension):
        (self.root / "Cargo.toml").write_text(f'[package]\nname = "dodo"\nversion = "{cargo}"\n')
        package = self.root / "editor-support/dodo-vscode/package.json"
        package.parent.mkdir(parents=True, exist_ok=True)
        package.write_text(json.dumps({"name": "dodo-vscode", "version": extension}))

    def test_extracts_matching_release_notes(self):
        version, notes = check_release.check(self.root, "v1.2.3")
        self.assertEqual(version, "1.2.3")
        self.assertEqual(notes, "Summary paragraph.\n\n- First change.\n")

    def test_rejects_malformed_tags(self):
        for tag in ("1.2.3", "v1.2", "release-1.2.3", "v1.2.3 "):
            with self.subTest(tag=tag), self.assertRaisesRegex(ValueError, "must look like"):
                check_release.check(self.root, tag)

    def test_rejects_cargo_version_mismatch(self):
        self.write_versions("1.2.4", "1.2.3")
        with self.assertRaisesRegex(ValueError, "Cargo.toml"):
            check_release.check(self.root, "v1.2.3")

    def test_rejects_extension_version_mismatch(self):
        self.write_versions("1.2.3", "1.2.2")
        with self.assertRaisesRegex(ValueError, "package.json"):
            check_release.check(self.root, "v1.2.3")

    def test_requires_dated_changelog_entry(self):
        (self.root / "CHANGELOG.md").write_text(CHANGELOG.replace("## 1.2.3 — 2026-10-01", "## 1.2.3"))
        with self.assertRaisesRegex(ValueError, "no dated entry"):
            check_release.check(self.root, "v1.2.3")

    def test_rejects_empty_changelog_entry(self):
        (self.root / "CHANGELOG.md").write_text("## 1.2.3 — 2026-10-01\n\n## 1.2.2 — 2026-09-01\n")
        with self.assertRaisesRegex(ValueError, "empty"):
            check_release.check(self.root, "v1.2.3")

    def test_accepts_prerelease_versions(self):
        self.write_versions("1.3.0-rc.1", "1.3.0-rc.1")
        (self.root / "CHANGELOG.md").write_text("## 1.3.0-rc.1 — 2026-10-05\n\n- Candidate.\n")
        self.assertEqual(check_release.check(self.root, "v1.3.0-rc.1"), ("1.3.0-rc.1", "- Candidate.\n"))


if __name__ == "__main__":
    unittest.main()
