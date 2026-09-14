#!/usr/bin/env python3
"""Check the built documentation's internal links using only Python's stdlib."""

from __future__ import annotations

from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit
import json
import re
import sys


DIST = Path(__file__).resolve().parent.parent / "docs" / "dist"
ROOT = DIST.parent.parent
SITE = "https://jotrorox.github.io"
BASE = "/dodo/"


class Page(HTMLParser):
    def __init__(self, path: Path):
        super().__init__(convert_charrefs=True)
        self.ids: set[str] = set()
        self.links: list[str] = []
        self.navigation: list[str] = []
        self.current_pages: list[str] = []
        self.in_navigation = False
        self.headings = 0
        self.feed(path.read_text(encoding="utf-8"))

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]):
        attributes = dict(attrs)
        if tag == "nav" and attributes.get("aria-label") == "Documentation":
            self.in_navigation = True
        if tag == "a" and self.in_navigation and attributes.get("href"):
            self.navigation.append(attributes["href"])
            if attributes.get("aria-current") == "page":
                self.current_pages.append(attributes["href"])
        if attributes.get("id"):
            self.ids.add(attributes["id"])
        if tag == "h1":
            self.headings += 1
        for key in ("href", "src"):
            if attributes.get(key):
                self.links.append(attributes[key])

    def handle_endtag(self, tag: str):
        if tag == "nav":
            self.in_navigation = False


def main() -> int:
    pages = {path: Page(path) for path in DIST.rglob("*.html")}
    errors: list[str] = []
    checked = 0
    if DIST / "index.html" not in pages:
        print("Missing docs/dist/index.html; run npm run build in docs first.", file=sys.stderr)
        return 1

    inventory = ROOT / "docs/src/content/docs/standard-library.md"
    documented = set(re.findall(
        r"`((?:core|alloc|std)/[a-z0-9_/]+)`", inventory.read_text(encoding="utf-8")
    ))
    modules = {
        source.relative_to(ROOT / "stdlib").with_suffix("").as_posix()
        for source in (ROOT / "stdlib").rglob("*.dodo")
    }
    for module in sorted(modules - documented):
        errors.append(f"standard-library.md: package missing from inventory: {module}")

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

    expected_navigation = {
        BASE + path.relative_to(DIST).as_posix().removesuffix("index.html")
        for path in pages if path.name == "index.html"
    }
    navigation_order: list[str] | None = None
    for path, page in pages.items():
        relative = path.relative_to(DIST).as_posix()
        current = BASE + relative.removesuffix("index.html")
        if page.headings != 1:
            errors.append(f"{relative}: expected one h1, found {page.headings}")
        if path.name == "index.html":
            if set(page.navigation) != expected_navigation:
                missing = sorted(expected_navigation - set(page.navigation))
                extra = sorted(set(page.navigation) - expected_navigation)
                errors.append(f"{relative}: navigation mismatch: missing {missing}, extra {extra}")
            if len(page.navigation) != len(set(page.navigation)):
                errors.append(f"{relative}: duplicate navigation entries")
            if page.current_pages != [current]:
                errors.append(f"{relative}: expected exactly one current-page navigation link")
            if navigation_order is None:
                navigation_order = page.navigation
            elif page.navigation != navigation_order:
                errors.append(f"{relative}: inconsistent navigation order")
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
    print(f"Checked {len(pages)} pages, navigation, and {checked} internal links, assets, and search targets.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
