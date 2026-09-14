# Tree-sitter Dodo

The syntax grammar used by [Dodo for Zed](../dodo-zed). It recognizes Dodo
declarations, return types, balanced groups, comments, literals, keywords, and
operators. Expression bodies intentionally remain permissive token sequences;
the compiler's LSP supplies syntax validation and semantic analysis.

With Node.js 22 or newer and a C compiler:

```sh
npm ci
npm test
```

`npm run generate` regenerates `src/parser.c`, `src/grammar.json`, and
`src/node-types.json` using the locked Tree-sitter CLI. Generated files and headers
are checked in so Zed can compile the grammar without Node.js. The parser uses
Tree-sitter ABI 14 for compatibility with Zed's supported grammar ABI versions.

`npm test` checks corpus trees and highlight assertions, parses the repository's
examples, standard library and `.dodo` test files, compiles every Zed query, and
checks outline names and ranges. Queries live in `../dodo-zed/languages/dodo` so
tests execute the same files shipped with the extension. After intentional syntax
tree changes, review `tree-sitter test --update` output before accepting updated
corpus expectations.

See [Dodo for Zed](../dodo-zed#install-from-this-checkout) for local installation.
The grammar and generated parser use Dodo's [BSD-2-Clause license](LICENSE).
The generated `src/tree_sitter` headers come from Tree-sitter 0.25.10 and use its
[MIT license](LICENSE.tree-sitter).
