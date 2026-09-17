#!/usr/bin/env python3
"""Generate searchable public API pages from the bundled Dodo source (stdlib only).

This deliberately extracts declarations, not function bodies. A small lexical mask
keeps braces/keywords inside strings and comments from affecting the scan. Unknown
public declaration forms fail the build instead of silently disappearing.
"""

from __future__ import annotations

import argparse
from bisect import bisect_left
from collections import defaultdict
from dataclasses import dataclass
import html
import json
from pathlib import Path
import posixpath
import re

ROOT = Path(__file__).resolve().parent.parent
STDLIB = ROOT / "stdlib"
OUTPUT = ROOT / "docs/src/content/docs/api"
REPOSITORY = "https://github.com/Jotrorox/dodo/blob/main/"
INTRINSIC_IMPORTS = {"core/mem", "core/ptr", "core/mmio"}
VIRTUAL_IMPORTS = {
    f"std/{family}/native"
    for family in ("platform", "fs", "env", "process", "thread", "sync", "net", "time", "tls", "web")
}


@dataclass
class Declaration:
    name: str
    kind: str
    signature: str
    comment: str
    line: int
    owner: str = ""


@dataclass(frozen=True)
class Token:
    value: str
    start: int
    end: int
    kind: str


# Dodo has line comments, strings, byte strings, and byte literals, but no
# block comments or character literals. Keep literal tokens opaque to parsing.
TOKEN = re.compile(
    r'(?P<comment>//[^\n]*)|'
    r'(?P<literal>b?"(?:\\[^\r\n]|[^"\\\r\n])*"|b\'(?:\\[^\r\n]|[^\'\\\r\n])*\')|'
    r'(?P<word>[A-Za-z_]\w*)|(?P<symbol>->|::|[^\s])'
)


def tokens(source: str, comments: bool = False) -> list[Token]:
    result = []
    for match in TOKEN.finditer(source):
        if match.lastgroup == "symbol" and match[0] in ('"', "'"):
            raise ValueError(f"Unterminated or unsupported literal at offset {match.start()}")
        if comments or match.lastgroup != "comment":
            result.append(Token(match[0], match.start(), match.end(), match.lastgroup or ""))
    return result


def lexical_mask(source: str) -> str:
    """Preserve offsets/newlines while removing comments and literal contents."""
    result = list(source)
    for token in tokens(source, comments=True):
        if token.kind in ("comment", "literal"):
            result[token.start:token.end] = " " * (token.end - token.start)
    return "".join(result)


def token_structure(items: list[Token]) -> tuple[dict[int, int], dict[int, int | None]]:
    """Match delimiters and identify each token's immediate enclosing brace."""
    pairs: dict[int, int] = {}
    parents: dict[int, int | None] = {}
    stack: list[int] = []
    braces: list[int] = []
    for index, token in enumerate(items):
        parents[index] = braces[-1] if braces else None
        if token.kind in ("comment", "literal"):
            continue
        if token.value in ("{", "(", "["):
            stack.append(index)
            if token.value == "{":
                braces.append(index)
        elif token.value in ("}", ")", "]"):
            expected = {"}": "{", ")": "(", "]": "["}[token.value]
            if not stack or items[stack[-1]].value != expected:
                raise ValueError(f"Unbalanced source delimiters at offset {token.start}")
            opening = stack.pop()
            pairs[opening] = index
            pairs[index] = opening
            if token.value == "}":
                braces.pop()
    if stack:
        raise ValueError("Unbalanced source delimiters")
    return pairs, parents


def leading_notes(source: str, start: int, items: list[Token] | None = None,
                  pairs: dict[int, int] | None = None) -> tuple[str, str]:
    if items is None:
        items = tokens(source, comments=True)
        pairs, _ = token_structure(items)
    assert pairs is not None
    index = bisect_left([token.start for token in items], start) - 1
    comments: list[str] = []
    attributes: list[str] = []
    boundary = start
    while index >= 0:
        token = items[index]
        if re.search(r"\n[ \t\r]*\n", source[token.end:boundary]):
            break
        if token.kind == "comment":
            # A trailing comment on the previous declaration is not our note.
            line_start = source.rfind("\n", 0, token.start) + 1
            if source[line_start:token.start].strip():
                break
            comments.append(token.value[2:].lstrip("/ "))
            first = index
        else:
            first = pairs[index] - 2 if token.value == ")" else index - 1
            if first < 0 or items[first].value != "@" or items[first + 1].kind != "word":
                break
            attributes.append(source[items[first].start:token.end])
        boundary = items[first].start
        index = first - 1
    return "\n".join(reversed(comments)), "\n".join(reversed(attributes))


def declaration_head(items: list[Token], start: int) -> tuple[str, str] | None:
    index = start + 1
    while index < len(items) and (items[index].value in ("unsafe", "extern")
                                 or items[index].kind == "literal"):
        index += 1
    if index + 1 < len(items) and items[index].value in ("fn", "struct", "enum", "const", "static"):
        if items[index + 1].kind != "word":
            raise ValueError(f"Missing declaration name at offset {items[index].end}")
        return items[index].value, items[index + 1].value
    return None


def declaration_end(source: str, items: list[Token], start: int, kind: str) -> int:
    """Return the token after a signature/field/constant, without its body.

    Newlines terminate declarations except inside delimiter groups or after a
    required continuation (notably `:` and `->`). Generic angle brackets only
    nest in type signatures; constant comparisons are ordinary expressions.
    """
    stack: list[str] = []
    continuations = {":", "->", "=", "&", "*", "!"}
    effects = {"from", "stores", "requires_plain"}
    for index in range(start, len(items)):
        token = items[index]
        if index > start and not stack:
            previous = items[index - 1]
            if ("\n" in source[previous.end:token.start]
                    and previous.value not in continuations and token.value not in effects):
                return index
        value = token.value
        if not stack and (value in (";", "}") or (kind == "field" and value == ",")):
            return index
        if not stack and kind == "fn" and value == "{":
            return index
        if token.kind == "literal":
            continue
        openings = {"(": ")", "[": "]", "{": "}"}
        if kind not in ("const", "static"):
            openings["<"] = ">"
        if value in openings:
            stack.append(openings[value])
        elif stack and value == stack[-1]:
            stack.pop()
    if stack:
        raise ValueError(f"Unclosed public declaration at offset {items[start].start}")
    return len(items)


def extract(source: str) -> list[Declaration]:
    items = tokens(source)
    pairs, parents = token_structure(items)
    notes = tokens(source, comments=True)
    note_pairs, _ = token_structure(notes)

    declarations: list[Declaration] = []
    containers: dict[int, str] = {}
    for index, token in enumerate(items):
        if token.value != "pub" or (parents[index] is not None and parents[index] not in containers):
            continue  # Private types and all function bodies are inaccessible.
        owner = containers.get(parents[index], "")
        head = declaration_head(items, index)
        if not head:
            if owner:
                continue  # Direct public fields were included with their type.
            raise ValueError(f"Unrecognized public declaration at line {source.count(chr(10), 0, token.start) + 1}")
        kind, name = head
        comment, attributes = leading_notes(source, token.start, notes, note_pairs)
        if kind in ("struct", "enum"):
            opening = next((i for i in range(index, len(items)) if items[i].value == "{"), None)
            if opening is None:
                raise ValueError(f"Missing body for {kind} {name}")
            closing = pairs[opening]
            header = source[token.start:items[opening].start].strip()
            if kind == "enum":
                signature = source[token.start:items[closing].end]
            else:
                fields: list[str] = []
                excluded: set[int] = set()
                for member in range(opening + 1, closing):
                    if parents[member] != opening or member in excluded:
                        continue
                    value = items[member].value
                    if value == "@":
                        end = member + 2
                        if end < closing and items[end].value == "(":
                            end = pairs[end] + 1
                        excluded.update(range(member, end))
                    elif value == "fn":
                        end = declaration_end(source, items, member, "fn")
                        if end < closing and items[end].value == "{":
                            end = pairs[end] + 1
                        excluded.update(range(member, end))
                    elif value == "pub" and declaration_head(items, member) is None:
                        end = declaration_end(source, items, member, "field")
                        if end <= member + 1 or any(item.value == "=" for item in items[member:end]):
                            raise ValueError(f"Unrecognized public field in {name}")
                        field_comment, field_attributes = leading_notes(source, items[member].start, notes, note_pairs)
                        if field_comment:
                            fields.extend("    // " + line for line in field_comment.splitlines())
                        if field_attributes:
                            fields.extend("    " + line.strip() for line in field_attributes.splitlines())
                        field = source[items[member].start:items[end - 1].end]
                        fields.extend("    " + line.strip() for line in field.splitlines())
                        excluded.update(range(member, end))
                private = any(i not in excluded and parents[i] == opening
                              and items[i].kind == "word" and items[i].value not in ("pub", "unsafe", "extern")
                              for i in range(opening + 1, closing))
                if private:
                    fields.append("    // Private implementation fields omitted.")
                signature = header + " {\n" + "\n".join(fields) + "\n}"
                containers[opening] = name
        else:
            end = declaration_end(source, items, index, kind)
            signature = source[token.start:items[end - 1].end].strip()
        if attributes:
            signature = attributes + "\n" + signature
        declarations.append(Declaration(name, kind, signature, comment,
                                        source.count("\n", 0, token.start) + 1, owner))
    return declarations


def module_path(path: Path) -> str:
    relative = path.relative_to(STDLIB)
    items = tokens(path.read_text(encoding="utf-8"))
    package = items[1].value if len(items) > 1 and items[0].value == "package" else None
    if package == path.parent.name and not path.parent.with_suffix(".dodo").is_file():
        return relative.parent.as_posix()
    return relative.with_suffix("").as_posix()


def extract_imports(source: str) -> list[tuple[str, str]]:
    """Return (local alias, import path), excluding comments and function bodies."""
    items = tokens(source)
    _, parents = token_structure(items)
    imports = []
    for index, token in enumerate(items):
        if token.value != "import" or parents[index] is not None:
            continue
        if index + 1 >= len(items) or not items[index + 1].value.startswith('"'):
            raise ValueError(f"Unrecognized import at offset {token.start}")
        path = json.loads(items[index + 1].value)
        alias = path.rsplit("/", 1)[-1]
        if index + 2 < len(items) and items[index + 2].value == "as":
            if index + 3 >= len(items) or items[index + 3].kind != "word":
                raise ValueError(f"Missing import alias at offset {token.start}")
            alias = items[index + 3].value
        imports.append((alias, path))
    return imports


def guide_for(module: str) -> str:
    if module.startswith("core/"):
        return "core"
    if module.startswith("alloc/"):
        return "allocation"
    family = module.split("/")[1]
    return {
        "arena_bytes": "bytes", "bytes_alloc": "bytes", "pool_bytes": "bytes",
        "io_alloc": "io", "fmt": "formatting", "fmt_alloc": "formatting",
        "text_alloc": "text", "text_shared": "text", "float_decimal": "text",
        "encoding": "json", "checksum": "hash", "fs": "filesystem",
        "env": "environment", "process": "processes", "thread": "threads",
        "sync": "synchronization", "net": "networking",
    }.get(family, family)


def relative_link(module: str, target: str) -> str:
    return posixpath.relpath(target, "api/" + posixpath.dirname(module))


def render_note(line: str) -> str:
    """Escape literal HTML while leaving Markdown code spans for the renderer.

    Entities are not decoded inside code spans: escaping `&T` there would
    incorrectly display `&amp;T` instead of the source's type spelling.
    """
    rendered = []
    position = 0
    for match in re.finditer(r"(?<!`)(`+)(?!`)(.*?)\1(?!`)", line):
        rendered.append(html.escape(line[position:match.start()], quote=False))
        rendered.append(match[0])
        position = match.end()
    rendered.append(html.escape(line[position:], quote=False))
    return "".join(rendered)


def render_module(module: str, sources: list[Path], order: int,
                  known_modules: set[str] | None = None) -> tuple[str, int]:
    sources = sorted(sources)
    contents = {path: path.read_text(encoding="utf-8") for path in sources}
    if known_modules is None:
        known_modules = {module_path(path) for path in STDLIB.rglob("*.dodo")}
    guide = guide_for(module)
    group = module.split("/")[0] if not module.startswith("std/") else "std/" + module.split("/")[1]
    lines = ["---", f"title: {json.dumps(module)}",
             f'description: "Public declarations, types, methods, and source contracts for {module}."',
             'section: "API reference"', f"order: {order}", f"navigationGroup: {json.dumps(group)}",
             f"source: {json.dumps(sources[0].relative_to(ROOT).as_posix())}", "---", "",
             f"[Library guide]({relative_link(module, guide + '.md')}) · "
             f"[API index and notation]({relative_link(module, 'stdlib-api.md')})", "",
             "This page is generated from the bundled library in this checkout. "
             "Signatures and adjacent source comments are reproduced below; "
             "the linked guide explains usage, storage, failures, and platform support.", "",
             "```dodo", f'import "{module}"', "```", ""]
    if module.endswith(("/linux", "/windows", "/native")) or module == "std/float_decimal":
        lines += ["> This is a provider or implementation package. Start with the linked "
                  "guide's application API; native declarations depend on the selected target.", ""]
    imports: dict[tuple[str, str], list[Path]] = defaultdict(list)
    for path, source in contents.items():
        for imported in extract_imports(source):
            if path not in imports[imported]:
                imports[imported].append(path)
    lines += ["## Names used in signatures", "",
              "Unqualified names denote this package's types (including other source files "
              "in the same package), language built-ins, or generic parameters such as `T`. "
              "Qualified names use the import aliases below. These aliases belong to the "
              "library source; import a dependency yourself to use its alias in your program.", ""]
    if imports:
        lines += ["| Alias | Package | Source file |", "| --- | --- | --- |"]
        for (alias, imported), paths in sorted(imports.items()):
            if imported in known_modules:
                target = "api/" + imported + ".md"
            elif imported in INTRINSIC_IMPORTS:
                target = "stdlib-api.md#compiler-intrinsics"
            elif imported in VIRTUAL_IMPORTS:
                target = "stdlib-api.md#virtual-imports-and-target-providers"
            else:
                raise ValueError(f"{module}: import {imported!r} has no API page or known compiler mapping")
            source_links = ", ".join(
                f"[{path.name}]({REPOSITORY}{path.relative_to(ROOT).as_posix()})" for path in paths
            )
            lines.append(f"| `{alias}` | [`{imported}`]({relative_link(module, target)}) | {source_links} |")
        lines += [""]
    else:
        lines += ["This package has no source imports.", ""]
    count = 0
    for path, source in contents.items():
        try:
            declarations = extract(source)
        except ValueError as error:
            raise ValueError(f"{path.relative_to(ROOT)}: {error}") from error
        count += len(declarations)
        for declaration in declarations:
            qualified = f"{declaration.owner}.{declaration.name}" if declaration.owner else declaration.name
            heading = "###" if declaration.owner else "##"
            label = {"fn": "Function", "const": "Constant", "static": "Static value"}.get(
                declaration.kind, declaration.kind.capitalize())
            lines += [f"{heading} `{qualified}`", "",
                      f"{label} · [Source]({REPOSITORY}{path.relative_to(ROOT).as_posix()}#L{declaration.line})", ""]
            if declaration.comment:
                # A blockquote keeps literal source notes distinct from authored contracts.
                lines += ["\n".join("> " + render_note(line)
                                     for line in declaration.comment.splitlines()), ""]
            fence = "`" * max(3, 1 + max((len(run) for run in re.findall(r"`+", declaration.signature)), default=0))
            lines += [fence + "dodo", declaration.signature, fence, ""]
            if declaration.kind == "fn" and re.search(r"\bpub\s+unsafe\b", lexical_mask(declaration.signature)):
                lines += ["Requires an `unsafe` context. Follow the source safety preconditions "
                          "and the linked guide before calling this API.", ""]
    return "\n".join(lines), count


def generate() -> tuple[dict[Path, str], int, int]:
    modules: dict[str, list[Path]] = defaultdict(list)
    for path in sorted(STDLIB.rglob("*.dodo")):
        modules[module_path(path)].append(path)
    files: dict[Path, str] = {}
    total = 0
    rows = []
    for order, (module, paths) in enumerate(sorted(modules.items()), 10):
        content, count = render_module(module, paths, order, set(modules))
        files[OUTPUT / (module + ".md")] = content
        total += count
        guide = guide_for(module)
        rows.append(f"| [`{module}`]({module}.md) | [{guide.replace('-', ' ').capitalize()}](../{guide}.md) | {count} |")
    files[OUTPUT / "index.md"] = "\n".join([
        "---", 'title: "All source packages"',
        'description: "Every bundled source package, with links to its declarations and usage guide."',
        'section: "API reference"', "order: 1", 'source: "scripts/generate_api_docs.py"', "---", "",
        f"The library contains **{len(modules)} source packages** and **{total} public "
        "type, function, method, and constant declarations**. Public fields and enum "
        "variants are included with their types. This index is rebuilt from source.", "",
        "Read [API notation, compiler intrinsics, and virtual imports](../stdlib-api.md) "
        "before looking up a declaration. The [task-based library index](../standard-library.md) "
        "helps you choose a package.", "",
        "## Package directory", "", "| Import / API page | Usage guide | Declarations |",
        "| --- | --- | ---: |", *rows, "",
    ])
    return files, len(modules), total


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if generated pages are missing or stale")
    args = parser.parse_args()
    files, modules, declarations = generate()
    stale = []
    for path, content in files.items():
        if path.exists() and path.read_text(encoding="utf-8") == content:
            continue
        if args.check:
            stale.append(str(path.relative_to(ROOT)))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8", newline="\n")
    for path in sorted(OUTPUT.rglob("*.md")):
        if path not in files:
            if args.check:
                stale.append(str(path.relative_to(ROOT)))
            else:
                path.unlink()  # Only obsolete generated Markdown beneath OUTPUT.
    if stale:
        print("API documentation is stale; run scripts/generate_api_docs.py:\n" + "\n".join(stale))
        return 1
    print(f"{'Checked' if args.check else 'Generated'} {modules} API packages, {declarations} declarations.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
