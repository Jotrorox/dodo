# Dodo documents

The cleaned language specification is available in three formats:

- [Markdown](language-spec-0.1.md), the canonical editable source.
- [Plain text](language-spec-0.1.txt).
- [PDF](language-spec-0.1.pdf), with searchable text and section bookmarks.

The specification includes the September 2026 ergonomics revision and marks
remaining unresolved questions. It describes the intended language, not a claim that every
feature is implemented. Consult the project README for compiler support.

[Ownership diagnostics and editor hovers](diagnostics-and-editors.md) documents
labeled errors, runnable and intentionally rejected examples, and `dodo lsp`.

Regenerate the text and PDF from the repository root with Python 3.10 or newer:

```sh
python3 scripts/render_spec.py
```

Check that committed outputs match their source:

```sh
python3 scripts/render_spec.py --check
```

The renderer has no third-party dependencies and produces deterministic files.
Its supported Markdown subset is intentionally limited to the forms in this
document. Edit the Markdown source, then regenerate both outputs.
