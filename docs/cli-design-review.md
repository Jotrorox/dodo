# CLI review and proposed user experience

Status: implemented. The audit below records the original CLI and resulting
design. See the maintained [command guide](src/content/docs/command-line.md)
and [manifest guide](src/content/docs/project-manifests.md) for current usage.
This review complements the [optional manifest proposal](project-manifest-proposal.md).

The existing command set is small and useful. Keep its commands and working
invocations, make their help specific, and add a few consistent conveniences.
Named targets should fit the current path-based CLI without making a positional
argument change meaning when a manifest appears.

## Existing options, checked against the implementation

The authoritative sources are [`src/main.rs`](../src/main.rs) and
[`src/test_runner.rs`](../src/test_runner.rs), including their parsers and
execution paths. Existing CLI tests were also reviewed. Representative commands
were probed against the available `target/debug/dodo`; that binary predates the
latest CLI source timestamps, so observations were cross-checked in the source
and are not presented as verification of a fresh build.

| Command | Current input and options |
| --- | --- |
| `build`, `compile` | Optional file/directory, default `main.dodo`; all compiler options below. Both names execute the same action. |
| `run` | Same input; compiler options except `--output` and explicit `--emit`; program arguments follow `--`. Only runs the host platform. |
| `check` | Same input; platform options apply. Rejects output, emit, and linker arguments, but accepts several unused build settings. |
| `test` | Optional file/directory, default recursive scan of `.`; dedicated runner options and a subset of compiler options. |
| `fmt` | Optional file/directory, default recursive scan of `.`; `--check`, `--stdout`, and `-` for stdin. |
| `lsp`, `--lsp` | Stdio language server; rejects source paths and compiler options. |
| `help`, `-h`, `--help` | Global help; `test --help` has a separate page. |
| `-V`, `--version` | Compiler version; also accepted after build/check/run. |

Compiler options today:

| Option | Current behavior and design decision |
| --- | --- |
| `-o`, `--output PATH` | Persistent build destination; keep. Duplicate occurrences are errors. |
| `--emit KIND` | `exe`, `obj`, `asm`, `llvm-ir`, `bitcode`; also accepts `object`, `assembly`, `ir`, `bc`. Keep aliases, document canonical values. |
| `-O`, `--opt-level LEVEL`, `-O2` | Levels 0–3; default 0. Keep and expose `--release` as a named preset. |
| `-g`, `--debug` | DWARF debug information, independently of optimization. Add to test and add `--no-debug` for overrides. |
| `--target TRIPLE` | LLVM platform triple. Preserve its meaning. |
| `--cpu NAME` | CPU selection; keep in command-specific platform options. |
| `--features LIST` | LLVM CPU features, not package/language features. Clarify the help label. |
| `--panic MODE` | `auto`, `hosted`, `trap`; no unwinding mode. Keep as an advanced runtime option. |
| `--panic-hook NAME` | Non-returning C ABI failure handler; last CLI panic selection wins. Keep advanced. |
| `--linker PATH` | Overrides `DODO_CC`, otherwise `cc`. Keep. |
| `--link-arg ARG` | Repeated arguments to the linker. Keep exact argv boundaries and compiler control of the output path. |

Test runner options today:

| Purpose | Options |
| --- | --- |
| Discovery/reporting | `--list`, `--doc`, `--no-doc`, `--allow-empty` |
| Selection | Repeatable `--filter TEXT` (OR), repeatable `--skip TEXT`, `--exact` |
| Ignored tests | `--ignored`, `--include-ignored` (mutually exclusive) |
| Execution | `--show-output`, `--fail-fast`, `--timeout SECONDS` (30 by default; 0 disables) |
| Compilation | `-O` / `--opt-level`, `--cpu`, `--features`, `--linker`, `--link-arg` |

Keep those meanings. In particular, `dodo test something` remains a path;
use `--filter something` for a name filter. `--exact` requires a filter, and
`--doc` conflicts with `--no-doc`. Test does not currently accept debug, platform,
emission, or panic options. It should gain debug and profiles, while retaining
its hosted platform and failure-reporting requirements.

## Concrete usability gaps

| Current behavior | Proposed behavior |
| --- | --- |
| `dodo help run` prints global help; even `dodo help nonsense` succeeds. | Resolve the requested command and show its help; reject unknown help subjects. |
| `run --help` and `fmt --help` show global compiler and formatter options together. | Dedicated help for every command, showing only relevant options. |
| `--opt-level=2`, `--timeout=5`, and `--link-arg=-s` are rejected. | Accept both `--name value` and `--name=value` consistently. |
| `check --target --help` consumes `--help` as a triple and reports an LLVM target error. | Recognize the missing typed value before entering LLVM. |
| `dodo biuld` reports only an unknown command. | Suggest `build` when the match is clear; never execute a guessed command. |
| `dodo run --port 8080` reports only an unknown option. | Explain that program arguments follow `--`, with an example. |
| `check --linker missing` succeeds without using the linker; panic/debug/optimization settings also have no checking effect. | Hide these legacy no-op options from check help and issue a concise compatibility notice when explicitly supplied. |
| `test -g` is rejected despite shared compilation support. | Support test debug information through the common compiler-option model. |
| A cross-platform `run` validates the input before rejecting the platform. | Report the incompatible command/platform before reading or checking sources. |

The no-op check options remain accepted during the compatibility period; do not
break existing build scripts merely to clean up help. Saved build settings do
not generate notices during checking. New profile flags are not added to check,
because profiles currently change only optimization and debug information.

## Everyday command design

Present `build` as the primary name throughout help and tutorials, and retain
`compile` as a supported alias. Keep `fmt`, `check`, `run`, and `test` unchanged.
Keep bare `dodo` as an informational command; it must not start a build or run.

```sh
dodo build                         # current folder's project or main.dodo
dodo build --release               # release profile, also without a manifest
dodo build examples/hello.dodo -g   # standalone file with debug information
dodo build -b server --release     # a named manifest target
dodo run -b server -- --port 9090   # program arguments replace saved defaults
dodo check -b wasm                 # check the named target's entry and platform
dodo test --release                # hosted tests using the release profile
dodo test -g --filter parser       # debug information for selected tests
dodo fmt --check                   # suitable for CI
dodo targets                       # list manifest targets
```

Use `-b NAME` as the short form of `--build-target NAME`; leave `--target TRIPLE`
untouched. Do not make `dodo build server` guess whether `server` is a directory,
file, or target. A named selector remains stable when files are added or removed.

Built-in `dev` and `release` profiles work with standalone files and ordinary
folders. A custom profile requires a manifest. This corrects the first draft's
unnecessary requirement for a manifest just to use `--release`.

Manifest-backed test builds have their own hosted settings, discovery root, and
timeout. They share profile selection and option syntax with application builds,
without inheriting an application's cross-platform linker or panic hook. The
manifest proposal specifies their resolution and discovery rules.

## Help and discovery

Top-level help should show a short description, command list, two or three
common examples, and how to get command help. Move the full compiler option
catalog into `dodo build --help`.

`dodo help run`, `dodo run -h`, and `dodo run --help` must show the same command
help. Help must not load a manifest, read source files, or probe toolchains.
Show everyday settings before advanced platform, linker, and panic settings.
Include defaults and constraints beside the relevant option.

For example, the opening of run help would be:

```text
Build a temporary executable and run it.

Usage: dodo run [FILE|DIRECTORY] [OPTIONS] [-- PROGRAM_ARGS...]

Examples:
  dodo run
  dodo run examples/hello.dodo
  dodo run -b server --release -- --port 8080

Common options:
  -b, --build-target NAME  Select a target from dodo.toml
      --release           Use the release profile
      --profile NAME      Select a profile (project default, otherwise dev)
  -g, --debug             Include source-level debug information
      --no-debug          Disable inherited debug information
  -O, --opt-level LEVEL   Override optimization: 0, 1, 2, 3

Program arguments after -- replace saved run.args; an empty -- clears them.
```

This is an opening excerpt, not the complete help page. Follow it with applicable
project, platform, linking, and runtime groups; omit `--output` and `--emit`.

## Predictable parsing and errors

Accept separated and equals forms of long value options. Preserve existing
`-O2`; additionally support attached `-oPATH` and `-bNAME` values. Preserve raw
`OsString` paths and program/linker arguments; require UTF-8 only where the
option's actual semantics require it.

Typed values such as levels, triples, and timeout must not accidentally consume
the following option. Raw `--link-arg` values may start with `-`; continue to
accept `--link-arg -s`, and show `--link-arg=-s` as an unambiguous spelling.
Use attached/equals values or a `./` prefix for paths beginning with a hyphen.
Help tokens after a run argument separator are program arguments, never Dodo
help requests. A help token used as an explicit linker-argument value also stays
a value. Preserve each command's existing `--` boundary semantics.

Keep repeatable linker arguments and test filters ordered. Preserve current
last-explicit-value behavior for scalar compiler options and the duplicate-output
error. Reject mutually exclusive new selectors with both option names in the
message. Do not silently correct misspellings, choose a target, or insert `--`.

Errors should identify the problem and provide an actionable next command:

```text
error: unknown build target 'sever'
  did you mean 'server'?
  available targets: inspect, server, wasm
  try: dodo build -b server

error: unexpected option '--port' for 'dodo run'
  program arguments must follow '--'
  example: dodo run -- --port 8080
```

For `--target server`, if `server` is a manifest target and not a valid LLVM
triple, explain the distinction and suggest `-b server`. For missing entries,
name the resolved path and manifest key. For an unsupported run target, suggest
the matching `dodo build -b NAME` invocation.

Validate parsing, selectors, and command/platform compatibility before loading
source code. Keep source/type errors separate from configuration/usage errors.

## Output and scripting

Add `-q` / `--quiet` and `-v` / `--verbose` to ordinary workflow commands, with a
conflict if both are supplied. Quiet suppresses progress and success summaries,
not compiler errors or a program's stdout/stderr. Verbose shows the selected
manifest, target/profile/platform, and linker invocation. These options do not
apply to the stdio LSP server or alter protocol output.

Build/check progress belongs on stderr. Primary data stays on stdout: help,
version, target listings, resolved configuration, formatter stdout, and program
stdout. Keep test listings/results as that command's primary report; send
compiler progress to stderr. This follows the separation recommended by the
[Command Line Interface Guidelines](https://clig.dev/#the-basics).

Moving current `Built ...` / `Checked ...` messages from stdout to stderr is an
observable change: update documentation and the CLI tests, including the
`Checked main.dodo` assertion in `tests/compiler.rs`, and call it out in release
notes. Avoid claiming complete byte-for-byte output compatibility.

Use exit 2 for invalid command-line usage and exit 1 for compilation/configuration
or test failures. `fmt --check` still returns 1 for formatting differences, and
`run` preserves the child's exit status. Preserve the LSP's separate protocol
exit rules. The distinction between usage failure and a child returning 2 is
communicated in diagnostics; do not rewrite a child's result.

## Implementation and delivery order

Move command definitions into `src/cli.rs` and share compiler argument groups
between build/run/test. Define which commands accept each option once, then
derive help and validation from that definition. Keep project resolution and
LLVM execution outside argument parsing, with explicit option presence retained.

The implementation uses an owned option registry for parsing, command help,
suggestions, and shell completion generation. Together with the owned TOML
parser, this meets the no-new-dependencies requirement.

Implement in this order:

1. CLI foundations: command help, long-option equals syntax, missing-value
   diagnostics, suggestions, shared parsing, and early command validation.
2. Manifest integration: `-b`, profiles (including standalone built-ins), hosted
   tests, negative/clearing overrides, target listings, and resolved-config
   inspection. Add quiet/verbose and document the output/exit-code changes.
3. Conveniences, also implemented: generated shell completions and `dodo init`
   for creating a small manifest/example without overwriting existing files,
   plus LSP target selection through the shared project resolver.

Acceptance checks should exercise the concrete gap table above, plus option
forms with spaces/non-UTF-8 paths, empty `--`, hyphen-prefixed linker arguments,
manifest overrides, check compatibility notices, test rerun commands, and
stdout/exit-status contracts. Avoid snapshots of entire help pages when a few
behavioral assertions can verify the contract.
