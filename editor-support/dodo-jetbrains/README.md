# Dodo for JetBrains IDEs

Language support for [Dodo](https://github.com/Jotrorox/dodo) in RustRover, CLion,
and other JetBrains IDEs, using the compiler's built-in `dodo lsp` language server.

- Syntax highlighting for declarations, types, attributes, strings and escapes,
  byte literals, numeric literals, borrowing, and operators, with a color settings page.
- Diagnostics, quick fixes, inferred type inlay hints, completion, hover
  documentation, go to declaration, find usages, rename, parameter info, and
  reformatting by the compiler's canonical formatter.
- Read-only bundled library sources with hover and navigation inside them.
- Comment toggling, brace and quote pairing, indentation, folding, and live
  templates generated from the VS Code extension's snippets.
- Unsaved file support, with compiler target and package-checking settings.

## Install

Install [Dodo](https://jotrorox.github.io/dodo/installation/) 0.1.6 or newer and
make sure `dodo --version` works. The plugin uses an existing compiler; it does not
contain the compiler or LLVM.

The plugin requires a 2026.2 or newer IDE that includes JetBrains' language server
support, such as RustRover or CLion. Download `dodo-jetbrains-0.1.6.zip` from
[GitHub Releases](https://github.com/Jotrorox/dodo/releases/tag/v0.1.6), then open
**Settings | Plugins**, click the gear icon, choose **Install Plugin from Disk...**,
and select the ZIP file without extracting it.

### Build from source

With JDK 25 or newer, run these commands from this repository's root:

```sh
cd editor-support/dodo-jetbrains
./gradlew buildPlugin
```

The plugin is written to `build/distributions/dodo-jetbrains-0.1.6.zip`. Building
it downloads CLion to compile against and does not require Rust or LLVM.

### Configure the compiler

Open **Settings | Languages & Frameworks | Dodo**. If Dodo is not on the IDE's
PATH, set **Server path** to the compiler executable, for example
`$PROJECT_DIR$/target/debug/dodo` for a source build (with `.exe` on Windows).
Relative paths, `$PROJECT_DIR$`, and `${workspaceFolder}` use the project
directory, and `~` is your home directory. Paths with spaces need no quoting.

On macOS, an IDE started from the Dock may not see a PATH set in your shell
profile; use an absolute path in that case.

## Settings

| Setting | Default | Meaning |
| --- | --- | --- |
| Enable the Dodo language server | On | Start the language server. |
| Server path | `dodo` | Executable on PATH or an absolute/relative compiler path. |
| Protocol trace | Off | Log LSP messages, or messages with their contents, in the tool window. |
| Check mode | File | Check each file and its imports, or use Package for all immediate sibling `.dodo` files. |
| Build target | Empty | Named target from the project's `dodo.toml`; empty uses its default. |
| Manifest path | Empty | Manifest path relative to the project directory, or absolute; empty discovers `dodo.toml`. |
| Target triple | Empty | LLVM triple override; empty uses the manifest target, otherwise the host. |

The server, path, and trace settings belong to your machine and are kept out of
version control. The compiler settings are stored in `.idea/dodo.xml`, so a team
can share them. Applying a change restarts the server.

Package checking requires the files in each directory to declare the same package.
Use the default file mode for folders containing independent example programs.

Use **Code | Reformat File** for the compiler's canonical four-space style. Enable
**Settings | Tools | Actions on Save | Reformat code** to format on save. The
compiler's style always applies; the IDE's code style settings only affect the
indentation of new lines while typing.

## Actions and live templates

**Tools | Dodo** provides **Restart Dodo Language Server** and **Show Dodo Language
Server Output**; both are also available from **Find Action**. The **Dodo Language
Server** tool window shows startup errors, server messages, and protocol traces.
The language services widget in the status bar shows the server's state and can
also restart it.

Type a template abbreviation and press Tab, or use **Code | Insert Live Template**:

| Abbreviations | Templates |
| --- | --- |
| `main`, `package`, `import` | Program and package setup |
| `fn`, `pubfn`, `method` | Functions and a borrowing method inside a struct |
| `struct`, `enum`, `interface`, `type` | Type declarations and aliases |
| `let`, `var`, `const` | Immutable, mutable, and constant bindings |
| `if`, `ifelse`, `for`, `foreach` | Conditionals and loops |
| `match`, `matchoption`, `matchresult` | Pattern matching |
| `printf` | Checked formatted console output |
| `unsafe`, `asm`, `interrupt` | Unsafe block, inline assembly, and an interrupt handler |
| `test`, `assert_eq` | Native tests and assertions |

Templates are not offered inside comments and strings. They are listed under
**Settings | Editor | Live Templates | Dodo**.

## Inlay hints and quick fixes

Inferred local types appear beside their binding names: `let answer = 42i32`
shows an `: i32` hint after `answer`. Explicit type annotations suppress the
corresponding hint. Hints follow unsaved edits and can be turned off in
**Settings | Editor | Inlay Hints**.

Place the caret on a diagnostic and press `Alt+Enter` (`Option+Return` on macOS).
For a direct assignment to, or mutable borrow of, an immutable local binding,
**Make `count` mutable** changes `let count = 1` to `count := 1`. For a missing
package qualifier such as `math.answer()`, **Import `math`** inserts the matching
import when the server can resolve a public member in a bundled library or nearby
local package. Quick fixes are normal editor edits: they can be undone and do not
save the file.

## Troubleshooting and limits

If the server fails to start, a notification links to the settings and the output.
Specify only the compiler executable: the plugin adds `lsp` automatically. After
installing or rebuilding Dodo, use **Restart Dodo Language Server**.

Highlighting, folding, and live templates work without a compiler. The language
server starts only in trusted projects, and starts as soon as you trust one.

The IDE's language server support only covers local files inside the project.
Files outside the project's content roots, scratch files, and other non-local files
get highlighting and editing support, but no diagnostics or code insight. Add their
folder to the project to analyze them.

Navigation and rename cover open documents and loaded imports. Bundled library
definitions open read-only sources supplied by the running compiler. Imported
`.dodo` files are watched for disk changes: creating, editing, or deleting a
dependency refreshes diagnostics and navigation. See the
[editor guide](https://jotrorox.github.io/dodo/editors/) for the server's current limits.

## Develop and test

The build uses the IntelliJ Platform Gradle Plugin. To compile and test against an
installed IDE instead of downloading CLion, pass its path:

```sh
./gradlew test -PlocalIdePath="$HOME/Applications/CLion.app"
./gradlew runIde -PlocalIdePath="$HOME/Applications/RustRover.app"
```

`runIde` starts the IDE with the plugin in a separate sandbox. The tests cover the
lexer against the repository examples, highlighting, editing behavior, folding,
live templates, settings, the protocol trace, and server path resolution.

`DodoLspIntegrationTest` drives the plugin against the real compiler, like the VS
Code extension's integration test. Build Dodo first with
`cargo build --locked --bin dodo` from the repository root; the test is skipped when
`target/debug/dodo` does not exist. Set `DODO_TEST_SERVER` to an absolute compiler
path to test another build.

Check compatibility with CLion and RustRover using the IntelliJ Plugin Verifier.
Without `verifyIdePaths`, it downloads both IDEs:

```sh
./gradlew verifyPlugin -PverifyIdePaths="$HOME/Applications/CLion.app,$HOME/Applications/RustRover.app"
```

Live templates are generated from `../dodo-vscode/snippets/dodo.json` at build time,
so both editors offer the same snippets. The lexer follows the compiler's
`src/lexer.rs`, and the highlighting rules follow the VS Code grammar; update the
tests when the compiler's syntax changes.

## License

[BSD-2-Clause](LICENSE), like Dodo.
