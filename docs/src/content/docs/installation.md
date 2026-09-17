---
title: "Install Dodo"
description: "Set up the compiler, choose a linker, and verify your installation on Linux or Windows."
section: "Start here"
order: 10
---

You need two tools to run a Dodo program: **Dodo** checks and compiles your
source, and a **C toolchain** links the generated code into an executable.
A prebuilt Dodo compiler includes LLVM and the standard library. You do not need
Rust or a separate LLVM installation unless you are building the compiler itself.

## Before you start

| What you want to do | Required tools |
| --- | --- |
| Check Dodo source or use editor diagnostics | The `dodo` executable. |
| Emit object files, assembly, LLVM IR, or bitcode | The `dodo` executable. |
| Run, test, or build an executable | Dodo plus a target-compatible C toolchain. |
| Build the Dodo compiler from source | Rust, LLVM development files, and a C toolchain; see [source builds](building-from-source.md). |

The published 0.1.3 archives target x86-64 Linux with glibc 2.39 or newer (such
as Ubuntu 24.04), and x86-64 Windows. The hosted standard library's supported
platforms are a separate question from LLVM's accepted code-generation targets;
see [platform support](standard-library.md#portable-and-hosted-functionality).

## Install the compiler

Download the matching archive from
[GitHub Releases](https://github.com/Jotrorox/dodo/releases) and extract it.
The examples below describe release 0.1.3. Documentation on `main` may contain
APIs added after that release.

### Linux

From the extracted directory containing `dodo`, install it into your user bin
folder and make it available in the current shell:

```sh
install -Dm755 dodo "$HOME/.local/bin/dodo"
export PATH="$HOME/.local/bin:$PATH"
dodo --version
```

The output begins with `dodo 0.1.3`. `PATH` is the list of directories your shell
searches for commands. The `export` above changes only this terminal. If your
shell does not already include `~/.local/bin`, add that export to its startup
configuration and open a new terminal to verify it persists.

### Windows

Extract the archive ending in `x86_64-pc-windows-msvc.zip` to a stable location,
for example `C:\Tools\Dodo`. In PowerShell, you can immediately run:

```powershell
& 'C:\Tools\Dodo\dodo.exe' --version
```

Adjust the path if the archive contains another directory level. To use the
short `dodo` command, open **Edit environment variables for your account**, edit
your user **Path**, and add the directory containing `dodo.exe`. Open a new
terminal and run:

```powershell
dodo --version
```

PowerShell requires `./` or `.\` when running an executable from the current
directory unless that directory is on PATH: use `.\dodo.exe --version` there.

If no archive matches your system, see [Build Dodo from source](building-from-source.md).
A successful compiler build alone does not add hosted-library support for a
new operating system or ABI.

## Install a C toolchain to run programs

Dodo invokes the linker driver named `cc` by default. You can choose another
with `--linker` for one command or `DODO_CC` for commands in the current environment.
You do not have to write C to use this toolchain.

### Linux toolchain

On Ubuntu:

```sh
sudo apt-get install build-essential
cc --version
```

On Fedora:

```sh
sudo dnf install gcc
cc --version
```

If your selected driver is Clang instead, use `dodo run --linker clang` or set
`export DODO_CC=clang` in your shell.

### Windows toolchain

Install Clang and the Visual Studio C++ Build Tools with a Windows SDK. Open an
x64 developer terminal for that toolchain so its libraries and headers are
available. Check Clang and select it for this PowerShell session:

```powershell
clang --version
$env:DODO_CC = 'clang'
```

If Clang is outside PATH, give its full path:

```powershell
$env:DODO_CC = 'C:\Program Files\LLVM\bin\clang.exe'
```

For one command, use `dodo run --linker clang`. The environment assignment lasts
for the current terminal and child processes; repeat it in a new session or add
`DODO_CC` to your user environment once the path works.

## Verify the complete setup

Follow [your first program](first-program.md) to create `hello/main.dodo`.
From the `hello` folder, run:

```sh
dodo --version
dodo check
dodo run
```

The version confirms command discovery, `check` confirms source compilation,
and `run` confirms linking and execution. Expect `Hello, world!` from the last
command. You can use `check` while configuring the linker.

## Troubleshooting installation

| Symptom | What to check |
| --- | --- |
| `dodo` is not recognized / command not found | Run the executable by its full path, then fix PATH and reopen the terminal. |
| PowerShell does not find an executable in the current directory | Use `.\dodo.exe` rather than `dodo.exe`. |
| The compiler cannot execute `cc` | Install a C toolchain or select a working driver with `--linker` / `DODO_CC`. |
| Clang runs, but linking reports missing Windows libraries or headers | Install the C++ Build Tools and Windows SDK; use their x64 developer terminal. |
| The Linux binary reports a missing glibc version | Use a compatible system or build Dodo on your intended host. |
| A bundled package or method is unknown | Compare compiler version and documentation revision; update the compiler or use its matching tagged docs. |
| `main.dodo` cannot be found | Change to your project folder or pass the path to the source file. |

For target selection and linker arguments, see
[targets and linking](command-line.md#targets-and-linking). For a compiler built
from source, use the more detailed [build prerequisites](building-from-source.md).

## Next step

Continue to [your first program](first-program.md). Set up
[your editor](editors.md) after you have verified the command-line compiler.
