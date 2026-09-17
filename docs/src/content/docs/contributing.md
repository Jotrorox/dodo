---
title: "Edit these docs"
description: "Write documentation in Markdown, preview the Astro site locally, and publish through GitHub Pages."
section: "Project"
order: 300
---

The entire `docs/` directory is an Astro website. Documentation lives in
`docs/src/content/docs/` as ordinary Markdown files. The only direct npm
dependencies are Astro and its Markdown renderer. Search runs locally in the
browser.

There are three kinds of content. Edit tutorials and reference guides directly.
Edit the canonical specification only when its design requirements change.
For generated API pages under `api/`, edit the corresponding library declaration
or its adjacent source comment, then regenerate. Do not hand-edit generated
Markdown; it is ignored by Git and replaced by the next build.

## Choose the right place

| Change | Source to edit |
| --- | --- |
| A reader's first steps | `index.md`, `installation.md`, `first-program.md`, or a `Learn Dodo` chapter. |
| Explain a language rule | The relevant language reference page; cross-check the parser, checker, and tests. |
| Explain a library task or contract | The relevant standard-library guide. |
| Add or change a public API | Its source in `stdlib/`, its guide, and examples. The signature reference regenerates automatically. |
| Change API extraction or its layout | `scripts/generate_api_docs.py` and its regression tests. |
| Change the normative language design | `language-spec-0.1.md`, requirements/evidence, and relevant implementation notes. |
| Change site navigation | Page frontmatter, `src/lib/docs.ts`, and the content schema when adding sections. |

## Edit or add a page

Edit an existing `.md` file, or add a new one under `docs/src/content/docs/` with
this frontmatter:

```yaml
---
title: "Your page title"
description: "A short description of what readers will learn."
section: "Using Dodo"
order: 140
---
```

Give each page one purpose: a task readers can complete or a topic they can look
up. Split longer guides into separate pages when readers can use those pages
independently. Choose a short, descriptive filename such as `compiler-options.md`.

The layout supplies the page title, so start body sections with `##` and use
`###` for subsections. Use ordinary Markdown paragraphs, lists, tables, links,
and fenced code blocks. Label Dodo code fences with `dodo`. Keep introductory
examples short and include the command to run them and the expected result.

Mark complete runnable examples with `dodo test` on the opening code fence.
Include a package declaration and a `main` or test function. Run
`dodo test docs --doc` from the repository root to verify them; compiler CI runs
the same command. Leave illustrative or incomplete snippets marked `dodo`.
See [executable documentation](testing.md#execute-documentation-examples).

For standard-library guides, start with the module's purpose, one recommended
API, a complete copyable program, its run command, and expected stdout or exit
status. Explain imports and storage beside first use. Handle likely failures
and tell readers how to recover. Put ownership, invalidation, numerical,
platform and protocol contracts after the quickstart; preserve them when
reorganizing a page. Use the current source APIs, including convenience methods.

Mark file/peer/credential-dependent examples `dodo` and include their complete
local setup and cleanup. Use loopback peers instead of public services. Validate
these separately at O0 and O3; do not mark an example executable merely to make
discovery count it. A silent successful example should assert its result and
explain what exit 0 proves. Use a fresh loopback port for each automated peer, as
`scripts/test_http_web.py` does; a recently closed fixed port may still be
reserved by the OS. Do not use `@test`-only code with a `dodo run` command.
The test runner checks exit status and assertions, not prose about stdout; compare
observable output separately when changing a quickstart.

Keep the [package inventory](standard-library.md#packages) in sync with `stdlib/`
and compiler intrinsic/virtual packages. Link every public family and identify
helper packages. Explicitly distinguish portable imports from hosted adapters
and accepted compilation targets from targets tested by execution.

## Place a page in navigation

The sidebar groups pages by their `section` field in this fixed order:

| Section | Purpose | Suggested `order` values |
| --- | --- | --- |
| `Start here` | Overview, installation, and a first program. | 0–99 |
| `Learn Dodo` | Sequential lessons, exercises, and a complete small application. | 0–99 |
| `Using Dodo` | Practical guides for everyday compiler use. | 100–199 |
| `Language reference` | Language topics and the full specification. | 200–299 |
| `Standard library` | Task-first module guides and package inventory. | 140–199 |
| `API reference` | API notation, generated package directory, and declarations. | Generated |
| `Project` | Implementation, requirements, and contributing. | 300 and above |

Within a section, the numeric `order` controls the page position. Leave gaps
between values so a new page can fit between existing ones. Pages with the same
order sort by title. Previous and next links follow the same sequence.

Generated API pages also set `navigationGroup` to keep the long package directory
in expandable sidebar groups. The current page's group opens automatically.
Their `source` field makes the edit link lead to the library source instead of
an ignored generated file.

Use one of the section names exactly as written above. Pages without a `section`
fall back to `Project` for compatibility; set it explicitly on new pages.

## Link pages and downloads

`index.md` is the homepage; other filenames become page URLs. Use Markdown
filenames for links between pages, such as
`[Language specification](language-spec-0.1.md)`. To link to a heading, append its
anchor: `language-spec-0.1.md#appendix-c-open-specification-items`.
Use full GitHub URLs for compiler source and examples outside the website.

Put downloadable files in `docs/public/downloads/` and link to them as
`/downloads/filename.ext`. The site handles the GitHub Pages path prefix.

## Preview locally

Install Node.js 24 and Python 3.10 or newer, then run from the repository root:

```sh
cd docs
npm ci
npm run dev
```

Open the local URL printed by Astro. Markdown edits update the preview.

The `predev` and `prebuild` scripts invoke `python3` to render the specification
and generate the API pages before Astro starts. If Windows provides Python as
`python` instead, run those preparation commands explicitly and start Astro
directly from `docs/`:

```powershell
python ../scripts/render_spec.py
python ../scripts/generate_api_docs.py
npx astro dev
```

Use `npx astro build` for the production build after the same preparation.
This is also useful when Python is selected by a full executable path.

## Check the production website

From `docs/`, build the site and check its generated links:

```sh
npm run build
python3 ../scripts/check_docs.py
npm run preview
```

Verify generated source and extraction behavior from the repository root:

```sh
python3 scripts/generate_api_docs.py --check
python3 scripts/test_generate_api_docs.py
dodo test docs --doc
dodo test docs --doc -O 3
```

The executable checks need a matching Dodo compiler and C linker. Use
`--linker PATH` or `DODO_CC` when the driver is not named `cc`. Example discovery
alone (`--list`) does not check or run a program. Test intentionally invalid
examples separately as rejections; do not mark them as executable success tests.

The build generates static pages, specification downloads, and a search index
in `docs/dist/`. The link checker validates internal pages, heading anchors,
assets, and search targets using the production `/dodo/` path prefix. Run it
after a fresh build so it checks your latest changes. It also checks that each
page has one main heading, appears in search and navigation, and has exactly one
current-page link. It also checks that every source package in `stdlib/` is
named in the overview inventory. It checks consistent sidebar ordering and duplicates; it does not check external
websites.

Open the preview URL to check the page layout, grouped navigation, search, and
light and dark themes. Check the mobile menu at a narrow window width when
changing the layout. Search updates automatically when content changes and runs
in the browser without an external service.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+K` / `Cmd+K` or `/` | Open search. |
| `↑` / `↓` | Select a search result. |
| `Enter` | Open the selected result. |
| `Escape` | Close the dialog. |
| `?` | Show keyboard shortcut help. |

The `/` and `?` shortcuts are ignored while you are typing in an input field.

## Update the specification downloads

The canonical specification is `docs/src/content/docs/language-spec-0.1.md`.
Edit that Markdown file; the website build automatically regenerates its plain
text and PDF downloads. Generated files are ignored by Git, so only the source
edit needs committing. To generate or check the downloads separately, run from
the repository root:

```sh
python3 scripts/render_spec.py
python3 scripts/render_spec.py --check
```

The renderer uses only Python's standard library and produces deterministic files
in `docs/public/downloads/`. It supports the Markdown forms used by the
specification; keep the frontmatter title on one line. When changing a language
rule, update its acceptance, rejection, or execution evidence in Appendix B.
Keep the implementation-defined choices and excluded features in Appendix C
distinct from the normative source-language requirements.

## Regenerate the API reference

From the repository root:

```sh
python3 scripts/generate_api_docs.py
python3 scripts/generate_api_docs.py --check
```

The generator groups directory packages, extracts public declarations, fields,
variants, attributes, and adjacent source comments, and writes one page per
source import under `docs/src/content/docs/api/`. It also writes the package
directory. It does not execute library code or copy function bodies. The build
fails on unrecognized public declaration forms instead of silently omitting
them. Compiler intrinsics and virtual imports live in the authored
[API overview](stdlib-api.md).

Keep source comments useful to callers: explain constraints, failure state,
ownership transfer, invalidation, and unsafe preconditions. The generator cannot
invent behavioral documentation from a signature. Put worked examples and
broader explanations in the guide, with links to the exact API page where useful.

The API Markdown, specification downloads, `docs/.astro/`, and `docs/dist/` are
generated and ignored by Git. A fresh checkout's `npm run build` recreates them.
Library-source changes trigger the documentation workflow so the published
reference follows the code.

## Publish

Push changes to `main` to build and deploy the website with GitHub Actions.
Pull requests build the site for validation. CI publishes the generated website
directly to this repository's [GitHub Pages](https://jotrorox.github.io/dodo/)
using the built-in GitHub Actions deployment permissions.

Publishing is already configured; routine documentation edits only need a
Markdown change and a push. The
[documentation workflow](https://github.com/Jotrorox/dodo/blob/main/.github/workflows/docs.yml)
defines the build and deployment steps.
