#!/usr/bin/env python3
"""Stage a Zed dev extension using an immutable snapshot of the local grammar."""

import json
from pathlib import Path
import re
import shutil
import subprocess


def git(repository, *args):
    return subprocess.check_output(
        ["git", "-C", str(repository), *args], text=True
    ).strip()


def main():
    source = Path(__file__).resolve().parent
    build = source.parents[1] / "build"
    destination = build / "dodo-zed"
    grammar = source.parent / "tree-sitter-dodo"
    snapshot = build / "tree-sitter-dodo"

    if not (grammar / "src" / "parser.c").is_file():
        raise SystemExit("Missing parser.c; run npm ci && npm run generate in editor-support/tree-sitter-dodo.")

    snapshot.mkdir(parents=True, exist_ok=True)
    if not (snapshot / ".git").exists():
        subprocess.run(["git", "init", "--quiet", str(snapshot)], check=True)
    # This private build directory contains only generated grammar snapshots.
    # Replacing src removes stale generated files after a grammar update.
    if (snapshot / "src").exists():
        shutil.rmtree(snapshot / "src")
    shutil.copytree(grammar / "src", snapshot / "src")
    for name in ("grammar.js", "LICENSE", "LICENSE.tree-sitter"):
        shutil.copy2(grammar / name, snapshot / name)
    git(snapshot, "-c", "core.autocrlf=false", "add", "src", "grammar.js", "LICENSE", "LICENSE.tree-sitter")
    changed = git(snapshot, "diff", "--cached", "--name-only")
    if changed:
        git(
            snapshot,
            "-c", "user.name=Dodo extension build",
            "-c", "user.email=build@localhost",
            "-c", "core.hooksPath=/dev/null",
            "commit", "--quiet", "--no-gpg-sign", "-m", "Snapshot local Dodo grammar",
        )
    revision = git(snapshot, "rev-parse", "HEAD")

    destination.mkdir(parents=True, exist_ok=True)
    for name in ("Cargo.toml", "Cargo.lock", "LICENSE", "LICENSE.tree-sitter", "README.md"):
        shutil.copy2(source / name, destination / name)
    for name in ("src", "languages"):
        if (destination / name).exists():
            shutil.rmtree(destination / name)
        shutil.copytree(source / name, destination / name)

    manifest = (source / "extension.toml").read_text()
    local_grammar = (
        "[grammars.dodo]\n"
        f"repository = {json.dumps(snapshot.as_uri())}\n"
        f"rev = {json.dumps(revision)}\n"
    )
    manifest, count = re.subn(
        r"(?ms)^\[grammars\.dodo\]\n.*?(?=^\[|\Z)",
        lambda _: local_grammar,
        manifest,
    )
    if count != 1:
        raise SystemExit("Expected exactly one [grammars.dodo] section in extension.toml")
    (destination / "extension.toml").write_text(manifest)
    print(f"Prepared {destination}")
    print(f"Grammar revision: {revision}")
    print("In Zed, run 'zed: install dev extension' and select the prepared directory.")
    print("After source changes, rerun this script and rebuild the dev extension in Zed.")


if __name__ == "__main__":
    main()
