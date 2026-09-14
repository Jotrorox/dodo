"""Exercise Zed's local grammar checkout workflow without changing a user profile."""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest


class PrepareDevTests(unittest.TestCase):
    def test_snapshot_is_fetchable_repeatable_and_tracks_uncommitted_changes(self):
        source = Path(__file__).resolve().parent
        with tempfile.TemporaryDirectory(prefix="dodo zed test ") as temporary:
            root = Path(temporary)
            extension = root / "editor-support" / "dodo-zed"
            grammar = extension.parent / "tree-sitter-dodo"
            extension.mkdir(parents=True)
            grammar.mkdir()
            for name in ("Cargo.toml", "Cargo.lock", "extension.toml", "LICENSE", "LICENSE.tree-sitter", "README.md", "prepare-dev.py"):
                shutil.copy2(source / name, extension / name)
            for name in ("src", "languages"):
                shutil.copytree(source / name, extension / name)
            for name in ("grammar.js", "LICENSE", "LICENSE.tree-sitter"):
                shutil.copy2(source.parent / "tree-sitter-dodo" / name, grammar / name)
            shutil.copytree(source.parent / "tree-sitter-dodo" / "src", grammar / "src")
            original = (extension / "extension.toml").read_bytes()

            def prepare():
                subprocess.run(
                    [sys.executable, str(extension / "prepare-dev.py")],
                    check=True, capture_output=True, text=True,
                )
                staged = root / "build" / "dodo-zed"
                self.assertEqual((extension / "extension.toml").read_bytes(), original)
                self.assertEqual((staged / "src/lib.rs").read_bytes(), (extension / "src/lib.rs").read_bytes())
                return tomllib.loads((staged / "extension.toml").read_text())["grammars"]["dodo"]

            first = prepare()
            self.assertTrue(first["repository"].startswith("file://"))
            self.assertEqual(len(first["rev"]), 40)
            self.assertNotIn("path", first)
            self.assertEqual(prepare(), first)

            checkout = root / "zed grammar checkout"
            subprocess.run(
                ["git", "clone", "--quiet", first["repository"], str(checkout)], check=True,
            )

            def checkout_revision(revision):
                subprocess.run(
                    ["git", "-C", str(checkout), "fetch", "--quiet", "--depth", "1", "origin", revision],
                    check=True,
                )
                subprocess.run(
                    ["git", "-C", str(checkout), "checkout", "--quiet", "--detach", revision],
                    check=True,
                )
                self.assertEqual((checkout / "src/parser.c").read_bytes(), (grammar / "src/parser.c").read_bytes())

            checkout_revision(first["rev"])
            with (grammar / "src/parser.c").open("a") as parser:
                parser.write("\n/* local grammar change */\n")
            second = prepare()
            self.assertEqual(second["repository"], first["repository"])
            self.assertNotEqual(second["rev"], first["rev"])
            checkout_revision(second["rev"])


if __name__ == "__main__":
    unittest.main()
