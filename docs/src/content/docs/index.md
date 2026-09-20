---
title: "Dodo documentation"
description: "Learn Dodo step by step, build useful programs, and look up the complete language and standard library."
section: "Start here"
order: 0
---

Dodo is a compiled systems language: you write `.dodo` source files, and the
compiler turns them into native programs. It checks types and borrowing before
your program runs. Memory ownership is explicit, resources are released when
their owners leave scope, and ordinary generated code needs no garbage collector
or scheduler.

You can begin with small terminal programs and fixed arrays. You do not need to
understand raw pointers, allocators, or hardware to get started.

## Start here

1. [Install Dodo](installation.md). Set up the compiler and linker for your system.
2. [Write your first program](first-program.md). Print a message, change it, check
   it, and keep an executable.
3. [Learn variables and expressions](language-basics.md). Work with values,
   numbers, strings, and arrays.
4. [Define types and functions](types-and-functions.md). Break a program into
   named operations and model data with structs and enums.
5. [Make decisions and repeat work](control-flow.md). Use `if`, loops, and `match`.
6. [Understand ownership](ownership.md) and [handle errors](patterns-and-results.md).
   Learn why values move, when references are valid, and how Results work.
7. [Reuse code with generics](generics.md), [split a project into packages](packages.md),
   and [write tests](testing.md).

The chapters include complete examples you can save and run. A **Try it** exercise
asks you to change working code; reference pages then explain the exact rules.
If a term is unfamiliar, use the [glossary](glossary.md).

<span id="use-dodo-in-a-project"></span>

## Choose a path

| You want to… | Go to |
| --- | --- |
| Learn from the beginning | [Your first program](first-program.md), then [language basics](language-basics.md) |
| Build a small program using several features | [Build a temperature report](practical-program.md) |
| Check, format, test, or debug code | [Command line](command-line.md), [testing](testing.md), [editor setup](editors.md) |
| Understand a compiler error | [Diagnostics and common mistakes](diagnostics-and-editors.md) |
| Find a library for a task | [Standard library](standard-library.md) |
| Look up a function, type, field, or method | [API reference](stdlib-api.md), [all packages](api/index.md) |
| Check an exact language rule | [Syntax](implementation-syntax.md), [ownership](ownership.md), [language specification](language-spec-0.1.md) |
| Work on the compiler or documentation | [Build from source](building-from-source.md), [edit these docs](contributing.md) |

## What you can build

The standard library includes portable byte and UTF-8 text processing, JSON,
collections, mathematics, hashing, time values, and protocol engines. Hosted
adapters add console I/O, files, environment variables, processes, threads,
synchronization, sockets, TLS, HTTP clients, and web servers.

Start with [console output](console.md), [text](text.md), and
[collections](collections.md). For an application, follow [reading files](filesystem.md),
[JSON](json.md), or [web routing](web.md). Each guide introduces storage and
failure handling before its detailed contracts. The
[package inventory](standard-library.md#packages) distinguishes portable code
from APIs that require a supported operating system.

<span id="look-up-language-behavior"></span>

## Which version these docs describe

The compiler release is **0.1.4** and the language design is **Dodo 0.1**. This
website follows the repository's `main` branch, which may contain changes after
that release. The library is embedded in your compiler; check `dodo --version`
when an example uses an API your binary does not recognize.

The tutorials and guides describe implemented behavior. The
[language specification](language-spec-0.1.md) also defines design requirements;
[implementation decisions and limits](implementation.md) records the supported
subset and differences. For an exact release, use its
[tagged source](https://github.com/Jotrorox/dodo/tree/v0.1.4) and
[release notes](https://github.com/Jotrorox/dodo/blob/main/CHANGELOG.md).

## How to use the examples

A complete example includes `package`, imports if needed, and either `main` or
named tests. Save each standalone program in its own file or fresh project
folder. Run the command shown beside it. Smaller snippets illustrate one rule
and may need surrounding code; deliberately invalid examples are labeled.

Fences marked `dodo test` are executable documentation. From a compiler checkout,
`dodo test docs --doc` checks them. File, network, and device examples explain
any extra setup instead of assuming an external service is available.

Use **Ctrl+K** or **Cmd+K** to search pages and API declarations. The sidebar
separates the learning sequence, practical tooling, language reference, library
guides, and exact API declarations.

<span id="work-on-dodo"></span>

## Read offline

The Markdown guides live in
[docs/src/content/docs](https://github.com/Jotrorox/dodo/tree/main/docs/src/content/docs).
Download the language specification as [plain text](/downloads/language-spec-0.1.txt)
or [PDF](/downloads/language-spec-0.1.pdf). Both are generated from the same
canonical Markdown. A [local site build](contributing.md#preview-locally) also
creates the full searchable library reference from the bundled sources.
