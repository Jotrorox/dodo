#!/usr/bin/env python3
"""Check a release tag against the package versions and changelog, and extract its notes."""

from pathlib import Path
import argparse
import json
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
TAG = re.compile(r"v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)")


def release_notes(changelog: str, version: str) -> str:
    """Return the body of the changelog section for version."""
    heading = re.compile(rf"## {re.escape(version)} — \d{{4}}-\d{{2}}-\d{{2}}")
    lines = changelog.splitlines()
    for index, line in enumerate(lines):
        if heading.fullmatch(line):
            body = []
            for following in lines[index + 1:]:
                if following.startswith("## "):
                    break
                body.append(following)
            notes = "\n".join(body).strip()
            if not notes:
                raise ValueError(f"CHANGELOG.md has an empty {version} entry")
            return notes + "\n"
    raise ValueError(f"CHANGELOG.md has no dated entry for {version} (expected '## {version} — YYYY-MM-DD')")


def cargo_version(manifest: str) -> str:
    """Return [package].version from Cargo.toml without requiring Python 3.11's tomllib."""
    section = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", manifest, re.MULTILINE | re.DOTALL)
    version = section and re.search(r'^version\s*=\s*"([^"]+)"', section.group(1), re.MULTILINE)
    if not version:
        raise ValueError("Cargo.toml has no [package] version")
    return version.group(1)


def check(root: Path, tag: str) -> tuple[str, str]:
    """Return the version and release notes for tag, or raise ValueError."""
    match = TAG.fullmatch(tag)
    if not match:
        raise ValueError(f"Release tag {tag!r} must look like v1.2.3")
    version = match.group(1)
    versions = {
        "Cargo.toml": cargo_version((root / "Cargo.toml").read_text()),
        "editor-support/dodo-vscode/package.json":
            json.loads((root / "editor-support/dodo-vscode/package.json").read_text())["version"],
    }
    for source, found in versions.items():
        if found != version:
            raise ValueError(f"Tag {tag} does not match version {found} in {source}")
    return version, release_notes((root / "CHANGELOG.md").read_text(), version)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", help="release tag, for example v0.1.4")
    parser.add_argument("--notes", type=Path, help="write the changelog entry to this file")
    args = parser.parse_args()
    try:
        version, notes = check(ROOT, args.tag)
    except ValueError as error:
        sys.exit(f"error: {error}")
    if args.notes:
        args.notes.write_text(notes)
    print(version)


if __name__ == "__main__":
    main()
