---
title: "Dodo documentation"
description: "Install Dodo, write your first program, and find guides to the language and compiler."
section: "Start here"
order: 0
---

Dodo is a small, ahead-of-time compiled systems language with checked borrowing
and explicit hardware access. Its compiler produces native executables, object
files, assembly, LLVM IR, and bitcode. Ordinary generated code needs no garbage
collector, heap allocator, scheduler, or Dodo runtime.

The current compiler release is **0.1.2**, implementing part of the **Dodo 0.1**
language design. See [implementation decisions and limits](implementation.md)
for supported features and remaining work.

## Start here

1. [Install Dodo](installation.md): get the compiler and check your setup.
2. [Write your first program](first-program.md): create a folder with `main.dodo` and run it.
3. [Use the command line](command-line.md): format, check, build, and choose output formats.

## Use Dodo in a project

Set up [editor diagnostics and hovers](editors.md), then explore
the [example programs](https://github.com/Jotrorox/dodo/tree/main/examples).
The command-line guide also covers [project folders](command-line.md#project-folders-and-source-files)
and [target selection](command-line.md#targets-and-linking).

## Look up language behavior

- [Language specification](language-spec-0.1.md): normative syntax, types,
  ownership rules, worked examples, conformance tests, and implementation-defined choices.
- [Implementation decisions and limits](implementation.md): the behavior of the
  current compiler, including differences from the broader design.
- [Syntax and expressions](implementation-syntax.md), [ownership and borrowing](ownership.md),
  and [patterns and Results](patterns-and-results.md): focused language references.
- [Implementation requirements](spec-requirements.md): a checklist for reviewing
  behavior and planning conformance tests.

## Work on Dodo

Follow [Build Dodo from source](building-from-source.md) for compiler prerequisites,
build profiles, and development checks. Follow [Edit these docs](contributing.md)
to add or improve a documentation page and preview the website locally.

## Read offline

Download the language specification as
[plain text](/downloads/language-spec-0.1.txt) or a
[PDF with searchable text and section bookmarks](/downloads/language-spec-0.1.pdf).
Both are generated from the same Markdown source used by this site.
