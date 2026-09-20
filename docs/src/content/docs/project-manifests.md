---
title: "Configure a project"
description: "Use an optional dodo.toml for named build targets, profiles, saved arguments, and hosted tests."
section: "Using Dodo"
order: 102
---

Add `dodo.toml` when you want to save compiler settings or build several entry
points. Projects without a manifest continue to use `main.dodo`. `dodo init`
creates a small manifest and entry file; `dodo init --manifest-only` adds a
manifest for an existing `main.dodo`. Neither command overwrites existing files.

## Define targets

```toml
schema = 1

[project]
default-target = "server"

[profiles.dev]
debug = true

[targets.server]
entry = "src/main.dodo"

[targets.server.run]
args = ["--port", "8080"]
cwd = "."

[targets.tools]
entry = "tools/main.dodo"

[targets.wasm]
entry = "src/arithmetic.dodo"
emit = "obj"
triple = "wasm32-unknown-unknown"
```

Each entry is a `.dodo` file with its own imports. Choose platform-compatible
code for the WebAssembly object. A manifest does not add neighboring source
files to an entry's import graph or change relative import resolution.

```sh
dodo targets
dodo build
dodo build -b tools --release
dodo check -b wasm
dodo build --all-targets
dodo run -- --port 9090
```

`-b NAME` is short for `--build-target NAME`. `--target TRIPLE` continues to mean
an LLVM platform triple. Positional inputs always mean file or directory paths.
`compile` remains an alias for `build`.

An explicit target wins over `project.default-target`. A single target is
selected automatically; multiple targets without a default need `-b` or
`--all-targets`. With no target declarations, an implicit `app` target uses
`main.dodo`, so `schema = 1` alone is a valid manifest. Test-only projects do not
need that entry to exist.

`--all-targets` is supported by build and check, in sorted target-name order.
All selected configurations and entries are validated before building starts.
A later compilation failure does not remove earlier successful artifacts.

## Find the manifest

No input uses `./dodo.toml`, if present. A directory input uses `dodo.toml` in
that directory. There is no parent-directory or global configuration search.
An explicit source file bypasses manifests, even if one exists beside it.

```sh
dodo build path/to/project
dodo build --manifest-path path/to/dodo.toml
dodo run example.dodo --release
dodo build --no-manifest
```

A present but invalid manifest is an error. `--no-manifest` explicitly bypasses
it. `--manifest-path` cannot be combined with `--no-manifest`, or with an input
path for build/check/run. Help and version commands never read manifests.

The owned TOML 1.0 parser adds no dependencies and limits nesting to 64 levels.
Duplicate keys, invalid types, unsupported schemas, and unknown settings report
the manifest path and source location. `[tool]` is reserved for arbitrary tool
data. Target and profile names use ASCII letters, digits, `_`, and `-`; names
must start with a letter or digit, avoid reserved Windows filenames, and must not
collide when compared without case.

## Shared settings and profiles

```toml
schema = 1

[build]
opt-level = 1
panic = "auto"
link-args = ["-lm"]

[profiles.release]
opt-level = 3
debug = false

[profiles.inspect]
opt-level = 1
debug = true

[targets.app]
entry = "main.dodo"
```

Settings resolve in this order: compiler defaults, `[build]`, the selected
target, the selected profile, then explicit CLI options. Profiles contain only
`opt-level` and `debug`. `--profile inspect` selects a custom profile;
`--release` selects `release`. Set `project.default-profile` to change the
manifest default from `dev`.

Built-in `dev` adds no overrides (compiler defaults are optimization 0 and debug
information off). Built-in `release` sets optimization 3 and debug information
off. Either can be customized in the manifest. Both work without a manifest;
custom profiles require one. `check` does not use profiles.

| Location | Supported fields |
| --- | --- |
| Root | Required `schema = 1`. |
| `[project]` | Display `name`, `default-target`, `default-profile`. |
| `[build]` | `out-dir` and compiler settings below. |
| `[targets.NAME]` | Required `entry`, optional `emit`, and compiler settings. |
| `[targets.NAME.run]` | Argument array `args`, working directory `cwd`. |
| `[profiles.NAME]` | `opt-level` and `debug`. |
| `[test]` | Discovery `root`, per-test `timeout` in seconds. |
| `[test.build]` | Hosted settings described below. |
| `[tool.NAME]` | Data reserved for other tools; Dodo does not interpret it. |

Compiler settings are `opt-level` (integer 0–3), `debug` (boolean), `triple`,
`cpu`, `features`, `panic`, `panic-hook`, `linker`, and `link-args`. Features are
an LLVM CPU-feature string such as `"+sse4.2,-avx"`. `emit` accepts `exe` (default),
`obj`, `asm`, `llvm-ir`, or `bitcode`.

`panic` accepts `auto`, `hosted`, or `trap`; `panic-hook` names a non-returning
C ABI handler. A table may set one of these, and a later layer replaces the
whole policy. `--panic trap` therefore clears an inherited hook.

Linker selection is `--linker`, then `DODO_CC`, then the resolved manifest
setting, then `cc`. A target's `link-args` replaces the shared array; `[]` clears
it. CLI `--link-arg` values append. `--clear-link-args` discards all inherited
arguments before appending CLI arguments. Non-executable builds require an
empty linker-argument list; checking never invokes a linker.

Use `--no-debug` to disable inherited debug information and `--features=""` to
clear CPU features. Each argument array element is one process argument. Dodo
does not expand environment variables, run a shell, or execute build hooks.

## Paths, execution, and artifacts

Manifest paths resolve relative to the manifest's directory. CLI paths resolve
relative to the caller. A bare linker name uses `PATH`; a linker path resolves
relative to where it was configured. A relative `DODO_CC` path uses the caller's
directory.

Manifest builds run the linker in the project root. Opaque linker arguments such
as `-Lnative` are interpreted relative to that root, whether supplied in TOML or
on the CLI. Manifest programs also start in the root unless `run.cwd` overrides
it. Standalone commands retain the caller's working directory.

Without `--`, run uses saved `run.args`. Arguments after an explicit `--` replace
them completely; `dodo run --` clears them. Run uses a temporary executable,
requires a host executable target, and preserves the program's exit status.

Manifest builds write:

```text
build/<profile>/<LLVM triple>/<target name><extension>
```

`build.out-dir` changes the base directory. Extensions are `.exe` for Windows
executables, `.o` for objects, `.s` for assembly, `.ll` for LLVM IR, and `.bc` for
bitcode. `--output PATH` specifies an exact destination with no suffix added.
Standalone builds retain `build/<project or source name>`. Output is replaced
only after successful compilation/linking; it cannot replace the selected
manifest or an existing Dodo source file.

## Configure hosted tests

```toml
schema = 1

[test]
root = "tests"
timeout = 15

[test.build]
debug = true
link-args = ["-lm"]
```

Tests resolve host defaults, then `[test.build]`, then the selected profile, then
CLI options. They do not inherit application `[build]` or target settings.
`[test.build]` accepts `opt-level`, `debug`, `cpu`, `features`, `linker`, and
`link-args`. Tests always use the host platform and the test runner's failure
reporting. Manifest test processes run in the project root.

```sh
dodo test -g
dodo test --release
dodo test --filter parser --timeout=5
dodo test tests/unit --manifest-path dodo.toml
```

Test root defaults to `.` and timeout to 30 seconds; 0 disables the timeout.
For test, an explicit path alongside `--manifest-path` overrides the discovery
root. Filters, skips, documentation and ignored-test selection remain explicit
CLI options. See [testing](testing.md) for their behavior.

## Inspect configuration and editor targets

`dodo build --print-config`, `dodo check --print-config`, and
`dodo test --print-config` report effective settings with their origins. They
create no artifacts and execute no linker or program. Test inspection does not
even discover tests. `--verbose` shows settings and subprocess invocations;
`--quiet` suppresses progress and success summaries.

The language server discovers `dodo.toml` in the workspace root. Use VS Code's
`dodo.buildTarget` to select a named target or `dodo.manifestPath` for another
manifest. An explicit `dodo.target` overrides the manifest's platform. Manifest
file-watch events refresh diagnostics; a malformed edit keeps the last valid
platform and reports the configuration error. See [editor setup](editors.md).
