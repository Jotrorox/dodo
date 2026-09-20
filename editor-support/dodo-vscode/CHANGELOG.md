# Changelog

## 0.1.4 — 2026-09-20

- Show inferred local types as inlay hints that follow unsaved edits.
- Add diagnostic quick fixes to make bindings mutable and import resolvable
  packages, using structured compiler diagnostics.
- Select named `dodo.toml` targets with `dodo.buildTarget` and configure manifest
  discovery with `dodo.manifestPath`. Reload platform settings when the manifest
  changes.
- Reject renames that would introduce name collisions.
- Expand editor integration coverage and document the new settings and actions.
- Require Dodo 0.1.4 for the new server features and publish the VSIX alongside
  the compiler in GitHub Releases.

## 0.1.3 — 2026-09-16

- Open bundled standard-library definitions as read-only documents with hover
  and navigation, and refresh them when the language server restarts.
- Refresh diagnostics and navigation when imported files change on disk,
  preserving unsaved editor buffers.
- Add printing completions, signature help, and a checked `printf` snippet.
- Update language-client and build dependencies, including TypeScript 7;
  require VS Code 1.137 or newer and Dodo 0.1.3 for the new server features.
- Publish the VSIX alongside the Dodo 0.1.3 compiler in GitHub Releases.

## 0.1.2 — 2026-09-14

- Publish the VSIX alongside the Dodo 0.1.2 compiler in GitHub Releases.
- Align the extension version and installation instructions with the compiler release.

## 0.1.0

- Connect to `dodo lsp` for diagnostics, completion, hover, definition, references,
  rename, signature help, and document formatting.
- Add Dodo syntax highlighting, bracket and comment support, and code snippets.
- Configure the compiler path, check mode, target triple, and protocol tracing.
- Add language server restart and output commands, VSIX packaging, and an F5 development setup.
