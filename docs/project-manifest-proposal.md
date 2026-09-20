# Optional project manifest proposal

Status: implemented. This document records the design; the maintained usage
guide is [Configure a project](src/content/docs/project-manifests.md).

Add an optional `dodo.toml` beside a project's sources. It describes named
build targets, shared compiler settings, build profiles, and default program
arguments. A project without this file keeps today's source-selection and build
defaults. CLI help, output, and parsing improvements are specified separately
in the review.

The main distinction is between **what to build** (a named target such as
`server`) and **how to build it** (a profile such as `release`). Keep the existing
`--target` flag for an LLVM platform triple; use `-b` / `--build-target` for a
manifest target. The accompanying [CLI review](cli-design-review.md) inventories
today's options and specifies command help, parsing, and diagnostics changes.

## Example

```toml
schema = 1

[project]
name = "hello-service"
default-target = "server"
default-profile = "dev"

[build]
out-dir = "build"
panic = "auto"

[profiles.dev]
opt-level = 0
debug = true

[profiles.release]
opt-level = 3
debug = false

[targets.server]
entry = "src/main.dodo"
emit = "exe"

[targets.server.run]
args = ["--port", "8080"]
cwd = "."

[targets.inspect]
entry = "tools/inspect.dodo"
emit = "exe"

[targets.wasm]
entry = "src/arithmetic.dodo"
emit = "obj"
triple = "wasm32-unknown-unknown"

[test]
root = "."
timeout = 30

[test.build]
debug = true
```

The three entry files above represent separate import graphs. In particular,
`src/arithmetic.dodo` must use APIs supported by its WebAssembly target. Entry
selection does not change Dodo's existing relative import rules or automatically
include neighboring files.

Usage:

```sh
dodo build                              # server, dev profile
dodo build --release                    # server, release profile
dodo run                                # passes --port 8080
dodo run -- --port 9090                  # replaces the saved program arguments
dodo run --                             # passes no program arguments
dodo build -b inspect
dodo build -b wasm --release
dodo check -b wasm                       # checks for the configured platform
dodo test --release                      # tests with the release profile
dodo build --all-targets
dodo targets                            # lists names, entries, and output kinds
dodo build --print-config                # prints the resolved configuration
```

A small manifest with one explicit target is:

```toml
schema = 1

[targets.app]
entry = "main.dodo"
```

## Discovery and compatibility

For `build`, `compile`, `check`, and `run`:

| Invocation | Manifest lookup |
| --- | --- |
| `dodo build` | Look for `./dodo.toml`. |
| `dodo build path/to/project` | Look for `path/to/project/dodo.toml`. |
| `dodo build path/to/file.dodo` | Use the explicit source file without a manifest. |
| `dodo build --manifest-path path/to/dodo.toml` | Require and use that exact file. |
| `dodo build --no-manifest` | Use today's `main.dodo` workflow. |

Do not search parent directories or merge global configuration in version 1.
This follows Dodo's existing explicit project-folder behavior. From a nested
directory, pass the project directory or `--manifest-path`.

An automatically discovered manifest that is unreadable, malformed, or uses an
unsupported schema is an error. Only an absent file permits the existing
manifest-free fallback. Help and version commands do not load a manifest.

For build/check/run, `--manifest-path` cannot be combined with a positional input.
It always conflicts with `--no-manifest`. Explicit source files cannot be
combined with `--build-target` or `--all-targets`. Those two selectors require a
manifest; without one, report the missing manifest and show how to select it.
The new `targets` command follows the same directory/manifest lookup rules and
requires a manifest.

`--release` and `--profile dev|release` also work without a manifest, including
for explicit files and with `--no-manifest`. Custom profiles require a manifest.
Profiles apply to build/run/test; `check` does not accept the new profile flags,
because profiles currently contain only code-generation settings. Standalone
builds retain today's output paths even when selecting a built-in profile.

`compile` and `build` remain aliases. Positional arguments remain paths: a folder
named `server` never competes with a build target named `server`.

## Schema

Use TOML with kebab-case field names and one supported filename. The implementation
uses an in-repository TOML 1.0 parser and typed validation with source locations,
without adding dependencies. Nesting is limited to 64 levels.

| Table | Fields |
| --- | --- |
| Root | Required integer `schema = 1`. |
| `[project]` | Optional display `name`, `default-target`, `default-profile`. |
| `[build]` | Optional `out-dir` and shared compiler settings listed below. |
| `[targets.NAME]` | Required `entry`; optional `emit` and compiler settings. |
| `[targets.NAME.run]` | Optional string array `args` and directory `cwd`. |
| `[profiles.NAME]` | Optional `opt-level` and `debug`. |
| `[test]` | Optional discovery `root` and per-test `timeout` in seconds. |
| `[test.build]` | Hosted test compiler settings, described below. |
| `[tool.NAME]` | Reserved extension data; opaque to the compiler. |

If no targets are declared, supply an implicit `app` target with
`entry = "main.dodo"`. This permits settings-only and test-only manifests;
`main.dodo` need only exist when a command actually selects it. If any targets
are declared, there is no implicit target. Each declared target has an explicit
`.dodo` source file entry, not a directory or glob. `emit` defaults to `exe` and
accepts the existing canonical values: `exe`, `obj`, `asm`, `llvm-ir`, and
`bitcode`. Static and shared libraries would require a separate compiler feature.

Target and profile names must match `[A-Za-z0-9][A-Za-z0-9_-]*`. Reject names that
collide under ASCII case folding or are reserved filesystem names on Windows,
because these identifiers also become output path components. Project `name`
is only a display label; it does not change package declarations or imports.

Shared and per-target compiler settings map to existing options:

| Manifest field | Type / constraint | Existing CLI equivalent |
| --- | --- | --- |
| `opt-level` | Integer, 0–3 | `-O` |
| `debug` | Boolean | `--debug` |
| `triple` | LLVM target triple string | `--target` |
| `cpu` | String | `--cpu` |
| `features` | LLVM feature string, e.g. `"+sse4.2"` | `--features` |
| `panic` | `"auto"`, `"hosted"`, or `"trap"` | `--panic` |
| `panic-hook` | C symbol string | `--panic-hook` |
| `linker` | Executable name or path string | `--linker` |
| `link-args` | Array of argument strings | Repeated `--link-arg` |

Require valid types, reject unknown fields outside `[tool]`, and diagnose unknown
target/profile names with available choices. Schema and field validation covers
the whole manifest; entry existence and platform validation apply only to
selected targets. Discover a missing linker when actually invoking it, as today.
An unselected firmware target must not require an installed firmware toolchain
just to build the server.

Avoid a generic `compiler-args` array. Typed fields let the compiler reject
mistakes such as `opt-level = 9`, preserve path origins, and explain overrides.
Argument arrays remain appropriate for the linker and the executed program.

## Target and profile selection

Choose a target in this order:

1. `-b NAME` / `--build-target NAME`.
2. `[project].default-target`.
3. The only target, if exactly one exists.
4. Otherwise, error with the available names and selection syntax.

`--all-targets` selects every target in sorted name order, and is valid only for
`build`/`compile` and `check`. It conflicts with `--build-target` and `--output`.
Resolve and validate every selected configuration before starting; then process
targets sequentially, stopping on the first failure. Previously completed target
artifacts remain valid. This is not a transaction across all outputs.

For build/run/test, choose a profile using `--profile NAME`, then
`[project].default-profile`, then `dev`. `--release` is shorthand for
`--profile release`; reject it alongside `--profile` rather than depending on
argument order. Check does not select or apply a profile.

`dev` and `release` exist without manifest declarations. The built-in `dev`
profile has no overrides, so it preserves compiler/shared/target defaults
(normally optimization 0, debug information off). The built-in `release` profile
sets optimization 3 and debug information off. Manifest definitions customize
these profiles field by field. Other declared profiles have no implicit
overrides. Profiles cannot inherit other profiles in version 1.

Profiles initially contain only optimization and debug settings. Platform,
entry, emission, and linker choices belong to targets or shared build settings.
This keeps a `release` profile reusable for both a server and an object target.

## Precedence and argument semantics

Resolve compiler settings from lowest to highest priority:

```text
compiler defaults -> [build] -> selected target -> selected profile -> CLI
```

A profile contributes only its defined fields, including the built-in release
overrides described above. An absent CLI option never overwrites a saved value.
For example, if a target sets optimization 1 and the selected profile sets 3,
the result is 3; adding `-O 2` makes it 2.

There are four explicit special cases:

- **Linker selection:** `--linker` > `DODO_CC` > resolved manifest `linker` > `cc`.
  This gives the existing machine-specific environment setting an explicit
  override role. Read the environment in the driver and pass it to the resolver.
- **Panic policy:** `panic` and `panic-hook` represent one setting. Reject both
  in the same TOML table. A later layer providing either replaces the complete
  policy, so `--panic trap` clears an inherited hook. Preserve the current
  last-option-wins rule between explicit CLI panic options.
- **Linker arguments:** a target's `link-args` replaces the shared array;
  `[]` clears it. Explicit `--link-arg` values append in order. Add
`--clear-link-args` to discard all inherited arguments before appending all
  CLI arguments, independent of where that switch appears.
- **Program arguments:** no CLI `--` uses `run.args`; an explicit `--` replaces
  the whole array with the following tokens. An empty `--` clears saved args.

Add `--no-debug` to reverse a saved `debug = true`; when repeated, the last
explicit `--debug`/`--no-debug` wins. Existing `--features ""` clears features.
There is no general concatenation, environment interpolation, tilde expansion,
shell splitting, or command substitution in manifest strings. Each argument
array element is exactly one process argument.

## Paths, working directories, and output

The manifest's parent directory is the project root. Resolve manifest paths
(`entry`, `out-dir`, `run.cwd`, and path-valued `linker`) against that directory.
Relative CLI paths, including `--manifest-path`, `--output`, and path-valued
`--linker`, resolve against the caller's working directory. Absolute paths remain
absolute. A bare linker name uses `PATH`; a linker value containing a path
separator is treated as a path. A relative path in `DODO_CC` uses the caller's
working directory.

Linker subprocesses run in the project root in manifest mode. Raw linker
arguments, from either TOML or the CLI, remain opaque: a token such as `-Lnative`
is interpreted by the linker relative to that root. Resolve compiler-owned
input, temporary, and output paths to absolute paths before spawning it.

Programs launched by `run` use the project root by default; `run.cwd` overrides
it. Program arguments remain opaque and are interpreted by the program.
Use subprocess `current_dir`, never a process-wide directory change. Without
a manifest, retain the caller working directory for linking and execution.

Default persistent artifacts use:

```text
<root>/<out-dir>/<profile>/<effective LLVM triple>/<target name><extension>
```

For example, on an x86-64 Linux GNU host:

```text
build/dev/x86_64-unknown-linux-gnu/server
build/release/x86_64-unknown-linux-gnu/server
build/release/wasm32-unknown-unknown/wasm.o
```

`out-dir` defaults to `build`. Use `.o`, `.s`, `.ll`, and `.bc` for compiler
artifacts, and `.exe` for Windows executable targets. A CLI `--output` is the
exact destination, with no suffix added. All effective platform triples must
be validated before being used as path components.

This layout separates targets, profiles, and platforms. It is not a build cache:
two invocations of one target/profile/platform with different CLI overrides can
replace the same artifact. Preserve staged output replacement on successful
builds and existing protections against overwriting source files. Also reject
an output path that would overwrite the selected manifest, including through a
symlink.

## Command-specific behavior

- `build`/`compile` use the fully resolved configuration. Non-executable output
  rejects a nonempty effective `link-args`, as today; an object target can clear
  shared arguments with `link-args = []`.
- `check` uses the selected entry and platform settings, including target
  pointer width. Saved emission, output, linking, and run settings do not make
  checking invalid and are not executed. Explicit CLI output/emission/link-arg
  options remain errors, matching today's CLI.
- `run` requires the selected target to emit an executable for the host. It
  still builds and cleans up a temporary executable and returns the program's
  exit status. Saved `out-dir` does not affect it. Explicit `--output` and
  `--emit` remain errors; cross-platform or object targets receive an actionable
  error directing the user to `build`.
- `targets` lists declared names, entries, output kinds, platforms, and the
  default marker, without loading sources or probing linkers.
- `--print-config` on build/check/run resolves and validates configuration, then
  exits without compiling, creating directories, linking, or running programs.
  Print a normalized TOML report including manifest path, selected target and
  profile, absolute paths, and effective options. Annotate values with their
  origins, such as `targets.server`, `profiles.release`, `DODO_CC`, or `CLI`.
  In manifest-free mode, omit the manifest and target-name fields; include a
  profile only when explicitly selected. For `check`, omit the profile and
  clearly mark saved build/run fields as unused by the command.
  For `--all-targets`, print each resolved target in deterministic order.
- `test` uses the dedicated hosted configuration below, with profiles and
  `--print-config`. It does not accept application target selectors.
- `fmt` retains its existing file selection and does not read manifests.
  `lsp` accepts project selection through initialization options and watches
  the selected manifest. Both receive command-specific help.

## Hosted tests

Include test configuration in the initial manifest implementation. `test.root`
defaults to `"."`, relative to the manifest. `test.timeout` defaults to 30 seconds,
accepts finite nonnegative values using today's validation, and uses 0 to
disable the deadline. `--timeout` overrides it. Keep filters, skips, ignored-test
selection, documentation selection, and `--allow-empty` on the CLI in version 1
so these per-invocation choices remain explicit.

`[test.build]` accepts `opt-level`, `debug`, `cpu`, `features`, `linker`, and
`link-args`. Resolve tests using:

```text
host compiler defaults -> [test.build] -> selected profile -> explicit CLI
```

Tests do not inherit `[build]` or application-target settings. They always use
the host platform, temporary executable output, and the test runner's failure
policy. Project `default-profile` applies; `-g` / `--debug`, `--no-debug`,
`--profile`, and `--release` become available on `test`. Linker environment
precedence and linker-argument clearing work exactly as for a build. Manifest
test subprocesses use the project root as their working directory; standalone
tests retain the caller's working directory.

With no path, test looks for `./dodo.toml` and uses its test root. With a directory
path, it looks for a manifest in that directory; absent one, it scans that
directory as today. An explicit source/Markdown file bypasses discovery. For
tests specifically, allow `dodo test tests --manifest-path dodo.toml`: the
positional path overrides the discovery root and remains relative to the caller.
This differs from build/run, where the positional input selects a project or
entry, rather than a test discovery scope.

`test --list` still discovers without compiling or requiring a linker.
`test --print-config` reports the resolved root, timeout, profile, hosted
settings, and CLI discovery/selection options without discovering tests. It
conflicts with `--list`, since each selects a different report. Failure rerun
hints must preserve manifest selection, effective test scope, profile, explicit
compiler overrides, and timeout, with shell-appropriate argument quoting.

## Implementation shape

Add `src/cli.rs` for the shared command/option definitions and `src/project.rs`
for the manifest types, parsing, discovery, and resolution.
Keep these modules independent of LLVM so parsing and merging can be tested with
`--no-default-features`. A small typed `CompilerSettings` representation should
carry optional fields and a single panic-policy enum; only the driver converts
the final result into `codegen::Options`.

Refactor the driver along this pipeline:

```text
CLI tokens -> explicit overrides + input selection
           -> manifest discovery and parsing, if applicable
           -> target/profile resolution with value origins
           -> command-specific validation
           -> existing package loading, checking, compilation, and execution
```

The important change in `src/main.rs` is to stop filling compiler defaults
while parsing CLI tokens. Retain presence information, including an explicitly
empty `--`, negative debug override, and options forbidden for a given command.
Keep the current manifest-free execution semantics as a covered path through
the driver. Do not convert TOML into synthetic CLI tokens and recursively parse
them: that loses setting origins and mixes defaults with explicit user choices.

Reuse `package::load_for_target`, `sema::check_for_target`, and the existing
compilation/staging code. Source import resolution needs no manifest-specific
changes. A manifest never triggers shell hooks, code generation scripts, or
dependency downloads.

Implementation acceptance checks should cover:

1. Existing CLI behavior with no manifest and explicit files beside a manifest.
2. Strict parsing, schema versions, selection ambiguity, and useful locations
   for malformed or unknown settings.
3. Profile/target/CLI precedence, environment linker overrides, panic-policy
   replacement, and clearing saved debug, linker, and program arguments.
4. Calls from another directory, paths containing spaces, child working
   directories, and profile/platform output separation.
5. Checking an executable target without a linker; rejecting cross-platform
   runs; building object targets; preserving previous artifacts on failure.
6. Listing and resolved-configuration inspection without subprocess execution,
   plus all-target validation before any build starts.
7. Hosted test settings, debug/profile propagation, test-root overrides, and
   copyable failure rerun hints. Include a project whose application target is
   freestanding to verify it does not alter the hosted test configuration.

## Follow-up scope

Ship the core manifest with build/check/run/test, profiles, listing, and resolved
configuration inspection, alongside the essential CLI improvements described
in the review. Do not add aliases, target inheritance,
workspaces, a dependency resolver, build hooks, or a cache in this change.

Project-aware LSP configuration also ships using the same resolver, with named
target selection and manifest change watching. It never invokes linkers or runs
programs. See [editor setup](src/content/docs/editors.md).

If repeated command combinations remain useful after this settles, introduce
argv-based aliases separately. Keep aliases distinct from build targets and
keep arbitrary task execution outside this manifest's initial scope.

## References

C3's [project configuration](https://c3-lang.org/build-your-project/project-config/)
provides the useful pattern of shared settings, named targets, and CLI
overrides. Its [build commands](https://c3-lang.org/build-your-project/build-commands/)
search upwards for a project file; this proposal deliberately keeps Dodo's
current explicit folder selection.

Cargo's [configuration reference](https://doc.rust-lang.org/cargo/reference/config.html)
provides the TOML/table model and command-alias precedent. This proposal uses
one optional project file rather than adopting Cargo's configuration hierarchy.

Repository behavior informing this proposal is documented in
[command-line usage](src/content/docs/command-line.md),
[projects and imports](src/content/docs/packages.md), and
[testing](src/content/docs/testing.md), and implemented in
[`src/main.rs`](../src/main.rs), [`src/test_runner.rs`](../src/test_runner.rs),
and [`src/codegen.rs`](../src/codegen.rs).
