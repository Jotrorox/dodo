#!/usr/bin/env python3
"""Check the built documentation's internal links using only Python's stdlib."""

from __future__ import annotations

from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit
import json
import sys


DIST = Path(__file__).resolve().parent.parent / "docs" / "dist"
SITE = "https://jotrorox.github.io"
BASE = "/dodo-docs/"


class Page(HTMLParser):
    def __init__(self, path: Path):
        super().__init__(convert_charrefs=True)
        self.ids: set[str] = set()
        self.links: list[str] = []
        self.headings = 0
        self.feed(path.read_text(encoding="utf-8"))

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]):
        attributes = dict(attrs)
        if attributes.get("id"):
            self.ids.add(attributes["id"])
        if tag == "h1":
            self.headings += 1
        for key in ("href", "src"):
            if attributes.get(key):
                self.links.append(attributes[key])


def main() -> int:
    pages = {path: Page(path) for path in DIST.rglob("*.html")}
    errors: list[str] = []
    checked = 0
    if DIST / "index.html" not in pages:
        print("Missing docs/dist/index.html; run npm run build in docs first.", file=sys.stderr)
        return 1

    def check(link: str, current: str, source: str):
        nonlocal checked
        target = urlsplit(urljoin(SITE + current, link))
        if target.scheme not in ("http", "https") or target.netloc != urlsplit(SITE).netloc:
            return
        checked += 1
        target_path = unquote(target.path)
        if target_path == BASE.rstrip("/"):
            target_path += "/"
        if not target_path.startswith(BASE):
            errors.append(f"{source}: URL escapes the GitHub Pages base: {link}")
            return
        file = DIST / target_path.removeprefix(BASE)
        if file.is_dir():
            file /= "index.html"
        if not file.is_file():
            errors.append(f"{source}: missing target: {link}")
        elif target.fragment and file in pages and unquote(target.fragment) not in pages[file].ids:
            errors.append(f"{source}: missing heading or anchor: {link}")

    for path, page in pages.items():
        relative = path.relative_to(DIST).as_posix()
        current = BASE + relative.removesuffix("index.html")
        if page.headings != 1:
            errors.append(f"{relative}: expected one h1, found {page.headings}")
        for link in page.links:
            check(link, current, relative)

    index = DIST / "search.json"
    if not index.is_file():
        errors.append("Missing search.json")
    else:
        entries = json.loads(index.read_text(encoding="utf-8"))
        if not isinstance(entries, list) or not entries:
            errors.append("Search index must contain a nonempty list of entries")
        else:
            indexed_paths: set[str] = set()
            for entry in entries:
                if not isinstance(entry, dict) or not all(
                    isinstance(entry.get(key), str) for key in ("title", "heading", "text", "url")
                ):
                    errors.append("Search entry must have title, heading, text, and url strings")
                    continue
                url = entry.get("url")
                if not isinstance(url, str) or not url.startswith(BASE):
                    errors.append(f"Search entry has an invalid URL: {url!r}")
                else:
                    indexed_paths.add(urlsplit(url).path)
                    check(url, BASE, "search.json")
            for path in pages:
                if path.name == "index.html":
                    url = BASE + path.relative_to(DIST).as_posix().removesuffix("index.html")
                    if url not in indexed_paths:
                        errors.append(f"Page is missing from search: {url}")

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Checked {len(pages)} pages and {checked} internal links, assets, and search targets.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
