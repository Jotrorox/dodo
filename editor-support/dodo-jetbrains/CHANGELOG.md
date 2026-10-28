# Changelog

## Unreleased

- Add the Dodo plugin for RustRover, CLion, and other JetBrains IDEs from 2026.2,
  with the features of the VS Code extension: highlighting with a color settings
  page, comment toggling, brace and quote pairing, indentation, folding, and live
  templates generated from the VS Code snippets.
- Connect to `dodo lsp` for diagnostics, quick fixes, inferred type inlay hints,
  completion, hover documentation, go to declaration, find usages, rename,
  parameter info, and reformatting. Bundled library sources open read-only, with
  hover and navigation inside them.
- Configure the compiler path, checking mode, target, build target, and manifest
  under **Settings | Languages & Frameworks | Dodo**, and inspect startup errors
  and protocol traces in the **Dodo Language Server** tool window.
- Test the plugin against the real compiler, and check compatibility with CLion
  and RustRover using the IntelliJ Plugin Verifier.
