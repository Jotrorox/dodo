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
| `Using Dodo` | Practical guides for everyday compiler use. | 100–199 |
| `Standard library` | Task-first module guides and package inventory. | 140–199 |
| `Language reference` | Language topics and the full specification. | 200–299 |
| `Project` | Implementation, requirements, and contributing. | 300 and above |

Within a section, the numeric `order` controls the page position. Leave gaps
between values so a new page can fit between existing ones. Pages with the same
order sort by title. Previous and next links follow the same sequence.

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

## Check the production website

From `docs/`, build the site and check its generated links:

```sh
npm run build
python3 ../scripts/check_docs.py
npm run preview
```

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

## Publish

Push changes to `main` to build and deploy the website with GitHub Actions.
Pull requests build the site for validation. CI publishes the generated website
directly to this repository's [GitHub Pages](https://jotrorox.github.io/dodo/)
using the built-in GitHub Actions deployment permissions.

Publishing is already configured; routine documentation edits only need a
Markdown change and a push. The
[documentation workflow](https://github.com/Jotrorox/dodo/blob/main/.github/workflows/docs.yml)
defines the build and deployment steps.
