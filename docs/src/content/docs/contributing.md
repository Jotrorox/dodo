---
title: "Edit these docs"
description: "Write documentation in Markdown, preview the Astro site locally, and publish through GitHub Pages."
order: 50
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
order: 60
---
```

Write the page content below the frontmatter. The layout supplies the page title,
so start body sections with `##`. Use ordinary Markdown paragraphs, lists,
tables, links, and fenced code blocks. Label Dodo code fences with `dodo`.

Pages appear automatically in navigation, ordered by their numeric `order`.
`index.md` is the homepage; other filenames become page URLs. Links between
pages can use their Markdown filenames, such as `[Language spec](language-spec-0.1.md)`.
Section links also work: append `#appendix-c-open-specification-items` to that
filename to link to the open questions. Use full GitHub URLs for compiler source
and examples outside the website.

Put downloadable files in `docs/public/downloads/` and link to them as
`/downloads/filename.ext`. The site handles the GitHub Pages path prefix.

## Preview locally

Install Node.js 24 and Python 3.10 or newer, then run from the repository root:

```sh
cd docs
npm ci
npm run dev
```

Open the local URL printed by Astro. Markdown edits update the preview. To
check the production website:

```sh
npm run build
npm run preview
```

The build generates static pages, specification downloads, and a search index
in `docs/dist/`. Search updates automatically when content changes; it runs in
the browser without an external service.

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
specification; keep the frontmatter title on one line. The specification describes
the intended language, so retain its open design items and distinguish them from
implemented compiler features.

## Publish

Push changes to `main` to build and deploy the website with GitHub Actions.
Pull requests build the site for validation. CI publishes the generated website
directly to this repository's [GitHub Pages](https://jotrorox.github.io/dodo/)
using the built-in GitHub Actions deployment permissions.

Publishing is already configured; routine documentation edits only need a
Markdown change and a push. The
[documentation workflow](https://github.com/Jotrorox/dodo/blob/main/.github/workflows/docs.yml)
defines the build and deployment steps.
