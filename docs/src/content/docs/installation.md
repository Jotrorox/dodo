---
title: "Install Dodo"
description: "Install a prebuilt compiler and the tools needed to run your first Dodo program."
section: "Start here"
order: 10
---

A prebuilt Dodo compiler includes LLVM. You do not need to install LLVM or set
LLVM environment variables to use it.

## Install the compiler

Download the archive for your system from
[GitHub Releases](https://github.com/Jotrorox/dodo/releases), then unpack it.
The Linux release targets x86-64 systems with glibc 2.39 or newer, such as
Ubuntu 24.04. On Linux, run this from the directory containing the extracted
`dodo` binary:

```sh
install -Dm755 dodo "$HOME/.local/bin/dodo"
export PATH="$HOME/.local/bin:$PATH"
dodo --version
```

For this release, the output should begin with `dodo 0.1.2`. The `export` command
updates the current terminal; if your shell does not already include
`~/.local/bin` on `PATH`, add that line to your shell's startup configuration as
well.

On x86-64 Windows, download the `x86_64-pc-windows-msvc.zip` archive and extract
it. Add the folder containing `dodo.exe` to your user `PATH`, then open a new
terminal and run `dodo --version`. You can also run `.\dodo.exe --version`
directly from the extracted folder in PowerShell.

If no prebuilt compiler matches your system, use the
[source build guide](building-from-source.md).

## Install a C toolchain to run programs

Checking source and emitting object files, assembly, LLVM IR, or bitcode need
no external compiler tools. Building an executable with `dodo build` or running
one with `dodo run` requires a C toolchain. Dodo uses `cc` by default.

On Ubuntu:

```sh
sudo apt-get install build-essential
```

On Fedora:

```sh
sudo dnf install gcc
```

Check that the driver is available:

```sh
cc --version
```

You can select another C linker driver with `--linker` or `DODO_CC`; see
[targets and linking](command-line.md#targets-and-linking).

On Windows, install Clang and the Visual Studio C++ Build Tools with a Windows
SDK. Select the Clang driver in PowerShell with `$env:DODO_CC = "clang"`, or
pass `--linker clang` when building or running a program. Checking source and
emitting objects from the Windows archive work without these tools installed.

## Next step

Continue to [your first program](first-program.md).
