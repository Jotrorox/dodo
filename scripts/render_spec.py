#!/usr/bin/env python3
"""Render the canonical specification to text and PDF using only Python's stdlib.

This deliberately small renderer handles the Markdown subset used by our spec:
headings, paragraphs, lists, fenced code, two-column tables, and horizontal rules.
PDFs use the standard built-in fonts; no TeX, browser, or font download is needed.
The output has deterministic metadata and uncompressed page streams, avoiding
byte differences between system zlib implementations.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import re
import sys
import textwrap


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "docs" / "src" / "content" / "docs" / "language-spec-0.1.md"
DOWNLOADS = ROOT / "docs" / "public" / "downloads"
PAGE_WIDTH, PAGE_HEIGHT = 595.28, 841.89  # A4, in PDF points
MARGIN = 52.0
CONTENT_WIDTH = PAGE_WIDTH - MARGIN * 2
BOTTOM = 56.0


def specification_source() -> str:
    """Use the website's title as the document heading, without a YAML dependency.

    The title must be a single-line plain, single-quoted, or JSON-quoted string.
    Other frontmatter belongs to the website and is omitted from the exports.
    """
    source = SOURCE.read_text(encoding="utf-8")
    metadata, separator, body = source.partition("\n---\n")
    if not metadata.startswith("---\n") or not separator:
        raise ValueError("Specification must start with YAML frontmatter")
    match = re.search(r"^title:[ \t]*(.+)$", metadata, re.MULTILINE)
    if match is None:
        raise ValueError("Specification frontmatter needs a single-line title")
    title = match.group(1).strip()
    if title.startswith('"'):
        title = json.loads(title)
    elif title.startswith("'") and title.endswith("'"):
        title = title[1:-1].replace("''", "'")
    if not isinstance(title, str) or not title or title in ("|", ">") or "\n" in title:
        raise ValueError("Specification title must be a nonempty single-line string")
    return f"# {title}\n\n" + body.lstrip("\n")


def plain(text: str) -> str:
    """Remove only the inline notation used by the source document."""
    text = re.sub(r"`([^`]+)`", r"\1", text)
    text = text.replace("**", "")
    return re.sub(r"(?<!\w)\*([^*]+)\*(?!\w)", r"\1", text)


@dataclass
class Block:
    kind: str
    lines: list[str]
    level: int = 0


def blocks(markdown: str) -> list[Block]:
    result: list[Block] = []
    paragraph: list[str] = []
    code: list[str] | None = None

    def flush() -> None:
        if paragraph:
            result.append(Block("paragraph", [" ".join(paragraph)]))
            paragraph.clear()

    for line in markdown.splitlines():
        if line.startswith("```"):
            flush()
            if code is None:
                code = []
            else:
                result.append(Block("code", code))
                code = None
            continue
        if code is not None:
            code.append(line)
            continue
        if not line.strip():
            flush()
        elif line.startswith("#"):
            flush()
            marker, title = line.split(" ", 1)
            result.append(Block("heading", [plain(title)], len(marker)))
        elif line == "---":
            flush()
            result.append(Block("rule", []))
        elif line.startswith("|"):
            flush()
            if result and result[-1].kind == "table":
                result[-1].lines.append(line)
            else:
                result.append(Block("table", [line]))
        elif re.match(r"(?:- |\d+\. |[A-D]\. )", line):
            flush()
            paragraph.append(line.strip())
        elif line.endswith("  "):
            flush()
            result.append(Block("paragraph", [line.strip()]))
        else:
            paragraph.append(line.strip())
    flush()
    if code is not None:
        raise ValueError("Unterminated code fence in specification")
    return result


def text_document(parsed: list[Block]) -> bytes:
    output: list[str] = []
    for block in parsed:
        if block.kind == "heading":
            title = block.lines[0]
            output.extend([title, ("=" if block.level < 3 else "-") * len(title)])
        elif block.kind == "code":
            output.extend("    " + line if line else "" for line in block.lines)
        elif block.kind == "rule":
            output.append("-" * 72)
        elif block.kind == "table":
            rows = table_rows(block.lines)
            widths = [max(len(row[i]) for row in rows) for i in range(2)]
            for index, row in enumerate(rows):
                output.append(f"{row[0]:<{widths[0]}}  {row[1]}")
                if index == 0:
                    output.append("-" * widths[0] + "  " + "-" * widths[1])
        else:
            for line in block.lines:
                line = plain(line)
                marker = re.match(r"(?:- |\d+\. |[A-D]\. )", line)
                prefix = " " * len(marker.group()) if marker else ""
                output.extend(textwrap.wrap(line, 80, subsequent_indent=prefix))
        output.append("")
    return ("\n".join(output).rstrip() + "\n").encode("utf-8")


def table_rows(lines: list[str]) -> list[list[str]]:
    return [
        [
            plain(cell.strip()).replace(r"\|", "|")
            for cell in re.split(r"(?<!\\)\|", line.strip("|"))
        ]
        for line in lines
        if not re.fullmatch(r"[|\s:-]+", line)
    ]


def width(text: str, size: float, font: str = "F1") -> float:
    if font == "F3":
        return len(text) * size * 0.6
    # Conservative Helvetica metrics; bold uses a small extra safety margin.
    units = 0
    for char in text:
        if char in " ilI.,'`:;!|":
            units += 280
        elif char in "frt()[]{}":
            units += 360
        elif char in "mwMW@%":
            units += 890
        elif char.isupper():
            units += 700
        else:
            units += 570
    return units * size / 1000 * (1.04 if font == "F2" else 1)


def wrap(text: str, available: float, size: float, font: str = "F1") -> list[str]:
    result: list[str] = []
    current = ""
    for word in text.split():
        # Conformance test names and source links must fit inside table columns.
        if width(word, size, font) > available:
            if current:
                result.append(current)
            current = ""
            for character in word:
                if current and width(current + character, size, font) > available:
                    result.append(current)
                    current = ""
                current += character
            continue
        candidate = f"{current} {word}" if current else word
        if current and width(candidate, size, font) > available:
            result.append(current)
            current = word
        else:
            current = candidate
    if current:
        result.append(current)
    return result or [""]


def encoded(text: str) -> str:
    # Standard PDF fonts support Windows-1252. Fail on unexpected glyphs instead
    # of silently dropping specification content.
    return "<" + text.encode("cp1252").hex() + ">"


class PdfObjects:
    def __init__(self) -> None:
        self.objects: list[bytes] = []

    def add(self, value: str | bytes = b"") -> int:
        self.objects.append(value.encode("ascii") if isinstance(value, str) else value)
        return len(self.objects)

    def set(self, index: int, value: str) -> None:
        self.objects[index - 1] = value.encode("ascii")

    def finish(self, root: int, info: int) -> bytes:
        document = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
        offsets = [0]
        for index, value in enumerate(self.objects, 1):
            offsets.append(len(document))
            document.extend(f"{index} 0 obj\n".encode("ascii"))
            document.extend(value + b"\nendobj\n")
        start = len(document)
        document.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode("ascii"))
        for offset in offsets[1:]:
            document.extend(f"{offset:010} 00000 n \n".encode("ascii"))
        document.extend(
            (f"trailer\n<< /Size {len(offsets)} /Root {root} 0 R /Info {info} 0 R >>\n"
             f"startxref\n{start}\n%%EOF\n").encode("ascii")
        )
        return bytes(document)


class PdfDocument:
    def __init__(self) -> None:
        self.pages: list[list[str]] = []
        self.bookmarks: list[tuple[str, int, float]] = []
        self.y = 0.0
        self.new_page()

    def new_page(self) -> None:
        self.pages.append([])
        self.y = PAGE_HEIGHT - 66
        self.draw_text("DODO  /  LANGUAGE SPECIFICATION 0.1", MARGIN, PAGE_HEIGHT - 30,
                       8, "F2", "0.30 0.36 0.42")
        self.pages[-1].append(
            f"0.80 0.84 0.87 RG 0.5 w {MARGIN} {PAGE_HEIGHT - 41} m "
            f"{PAGE_WIDTH - MARGIN} {PAGE_HEIGHT - 41} l S"
        )

    def ensure(self, height: float) -> None:
        if self.y - height < BOTTOM:
            self.new_page()

    def draw_text(self, text: str, x: float, y: float, size: float = 10.4,
                  font: str = "F1", color: str = "0.12 0.16 0.20") -> None:
        self.pages[-1].append(
            f"BT /{font} {size} Tf {color} rg 1 0 0 1 {x:.2f} {y:.2f} Tm "
            f"{encoded(text)} Tj ET"
        )

    def heading(self, title: str, level: int) -> None:
        if level == 2 and title in ("Contents", "1. Language overview"):
            self.new_page()
        size = {1: 26, 2: 15.0, 3: 11.5}.get(level, 11.5)
        lines = wrap(title, CONTENT_WIDTH, size, "F2")
        self.ensure(len(lines) * (size + 4) + 76)
        if level == 2:
            self.bookmarks.append((title, len(self.pages) - 1, self.y))
        self.y -= 10 if level < 3 else 5
        for line in lines:
            self.draw_text(line, MARGIN, self.y, size, "F2", "0.08 0.26 0.35")
            self.y -= size + 5
        self.y -= 5

    def paragraph(self, text: str) -> None:
        text = plain(text)
        marker = re.match(r"(?:- |\d+\. |[A-D]\. )", text)
        indent = 14 if marker else 0
        lines = wrap(text, CONTENT_WIDTH - indent, 10.4)
        self.ensure(min(len(lines), 2) * 14.3)
        for index, line in enumerate(lines):
            self.ensure(14.3)
            self.draw_text(line, MARGIN + (indent if index else 0), self.y)
            self.y -= 14.3
        self.y -= 6.5

    def code(self, lines: list[str]) -> None:
        size, leading = 8.25, 11.5
        longest = max((len(line) for line in lines), default=1)
        size = min(size, (CONTENT_WIDTH - 20) / max(longest, 1) / 0.6)
        if size < 6.5:
            raise ValueError("Code line too long for legible PDF; wrap the Markdown source")
        self.ensure(min(len(lines), 4) * leading + 12)
        self.y -= 5
        for line in lines:
            self.ensure(leading + 4)
            self.pages[-1].append(
                f"0.95 0.96 0.97 rg {MARGIN} {self.y - 3:.2f} "
                f"{CONTENT_WIDTH} {leading + 1} re f"
            )
            self.draw_text(line, MARGIN + 9, self.y, size, "F3")
            self.y -= leading
        self.y -= 11

    def table(self, source: list[str]) -> None:
        rows = table_rows(source)
        first_width = 185.0
        for index, row in enumerate(rows):
            if len(row) != 2:
                raise ValueError("Only two-column specification tables are supported")
            font = "F2" if index == 0 else "F1"
            columns = [wrap(row[0], first_width - 16, 9.4, font),
                       wrap(row[1], CONTENT_WIDTH - first_width - 16, 9.4, font)]
            row_height = max(map(len, columns)) * 12.7 + 9
            self.ensure(row_height + (20 if index == 0 else 0))
            if index == 0 or index % 2 == 0:
                self.pages[-1].append(
                    f"0.94 0.96 0.97 rg {MARGIN} {self.y - row_height + 9:.2f} "
                    f"{CONTENT_WIDTH} {row_height} re f"
                )
            for column_index, column in enumerate(columns):
                for line_index, line in enumerate(column):
                    self.draw_text(line, MARGIN + 7 + column_index * first_width,
                                   self.y - line_index * 12.7 - 3, 9.4, font)
            self.y -= row_height
        self.y -= 9

    def render(self, parsed: list[Block]) -> bytes:
        for block in parsed:
            if block.kind == "heading":
                self.heading(block.lines[0], block.level)
            elif block.kind == "paragraph":
                self.paragraph(block.lines[0])
            elif block.kind == "code":
                self.code(block.lines)
            elif block.kind == "table":
                self.table(block.lines)
            elif block.kind == "rule":
                self.y -= 10

        objects = PdfObjects()
        catalog, tree = objects.add(), objects.add()
        fonts = [objects.add(
            f"<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>"
        ) for name in ("Helvetica", "Helvetica-Bold", "Courier")]
        pages: list[int] = []
        for index, commands in enumerate(self.pages):
            commands.append(
                f"BT /F1 8 Tf 0.38 0.42 0.46 rg 1 0 0 1 {MARGIN} 31 Tm "
                f"{encoded('9 September 2026')} Tj ET"
            )
            commands.append(
                f"BT /F1 8 Tf 0.38 0.42 0.46 rg 1 0 0 1 {PAGE_WIDTH - MARGIN - 36} 31 Tm "
                f"{encoded(f'{index + 1} / {len(self.pages)}')} Tj ET"
            )
            stream = "\n".join(commands).encode("ascii")
            content = objects.add(
                f"<< /Length {len(stream)} >>\nstream\n".encode("ascii")
                + stream + b"\nendstream"
            )
            resources = " ".join(f"/F{i + 1} {obj} 0 R" for i, obj in enumerate(fonts))
            pages.append(objects.add(
                f"<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
                f"/Resources << /Font << {resources} >> >> /Contents {content} 0 R >>"
            ))
        objects.set(tree, f"<< /Type /Pages /Count {len(pages)} /Kids ["
                    + " ".join(f"{page} 0 R" for page in pages) + "] >>")
        outline = objects.add()
        entries = [objects.add() for _ in self.bookmarks]
        for index, (title, page, y) in enumerate(self.bookmarks):
            links = (f" /Prev {entries[index - 1]} 0 R" if index else "")
            if index + 1 < len(entries):
                links += f" /Next {entries[index + 1]} 0 R"
            objects.set(entries[index],
                        f"<< /Title {encoded(title)} /Parent {outline} 0 R{links} "
                        f"/Dest [{pages[page]} 0 R /XYZ null {y:.2f} null] >>")
        objects.set(outline, f"<< /Type /Outlines /Count {len(entries)} "
                    f"/First {entries[0]} 0 R /Last {entries[-1]} 0 R >>")
        objects.set(catalog, f"<< /Type /Catalog /Pages {tree} 0 R "
                    f"/Outlines {outline} 0 R /PageMode /UseOutlines /Lang (en) >>")
        info = objects.add(
            "<< /Title (The Dodo Programming Language - Specification 0.1) "
            "/Subject (Dodo language design specification) "
            "/Producer (Dodo dependency-free specification renderer) >>"
        )
        return objects.finish(catalog, info)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if generated documents differ")
    args = parser.parse_args()
    parsed = blocks(specification_source())
    outputs = {DOWNLOADS / "language-spec-0.1.txt": text_document(parsed),
               DOWNLOADS / "language-spec-0.1.pdf": PdfDocument().render(parsed)}
    if not args.check:
        DOWNLOADS.mkdir(parents=True, exist_ok=True)
    stale = False
    for path, content in outputs.items():
        if args.check:
            if not path.is_file() or path.read_bytes() != content:
                print(f"Out of date: {path.relative_to(ROOT)}", file=sys.stderr)
                stale = True
        else:
            path.write_bytes(content)
            print(f"Wrote {path.relative_to(ROOT)} ({len(content):,} bytes)")
    return int(stale)


if __name__ == "__main__":
    raise SystemExit(main())
