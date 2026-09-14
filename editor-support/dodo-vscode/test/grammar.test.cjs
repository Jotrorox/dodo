const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { before, test } = require("node:test");
const { Registry, INITIAL } = require("vscode-textmate");
const { loadWASM, OnigScanner, OnigString } = require("vscode-oniguruma");

const root = path.resolve(__dirname, "..");
let grammar;

before(async () => {
  const wasm = fs.readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm"));
  await loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
  const registry = new Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (patterns) => new OnigScanner(patterns),
      createOnigString: (text) => new OnigString(text),
    }),
    loadGrammar: async () => JSON.parse(fs.readFileSync(path.join(root, "syntaxes/dodo.tmLanguage.json"), "utf8")),
  });
  grammar = await registry.loadGrammar("source.dodo");
});

function scopesAt(line, text) {
  const offset = line.indexOf(text);
  assert.notEqual(offset, -1);
  return grammar.tokenizeLine(line, INITIAL).tokens.find((token) =>
    token.startIndex <= offset && token.endIndex > offset).scopes;
}

function hasScope(line, text, scope) {
  assert.ok(scopesAt(line, text).includes(scope), `${text} in ${line}: expected ${scope}`);
}

test("declarations, borrowing, attributes, and generic calls", () => {
  hasScope("package hello", "hello", "entity.name.namespace.dodo");
  hasScope("pub struct Reading {", "Reading", "entity.name.type.dodo");
  hasScope("pub fn view(&self) -> &i32 from(self) {", "view", "entity.name.function.dodo");
  hasScope("pub fn view(&self) -> &i32 from(self) {", "self", "variable.language.dodo");
  hasScope("pub fn view(&self) -> &i32 from(self) {", "from", "keyword.other.dodo");
  hasScope("@ignore(\"reason\")", "ignore", "storage.type.annotation.dodo");
  hasScope("identity::<Option<i32>>(some(42))", "identity", "entity.name.function.dodo");
  hasScope("identity::<Option<i32>>(some(42))", "Option", "support.type.dodo");
  hasScope("assert_eq(main(), 0)", "assert_eq", "support.function.builtin.dodo");
});

test("numeric bases, suffixes, and ranges keep their boundaries", () => {
  for (const number of ["0xffu8", "0o77", "0b1010u32", "1_000usize", "42i64"]) {
    hasScope(number, number, "constant.numeric.integer.dodo");
  }
  for (const number of ["1.25f32", "2e-3", "42f64", "1_000.25e+2f64"]) {
    hasScope(number, number, "constant.numeric.float.dodo");
  }
  const ranges = grammar.tokenizeLine("0..10 0..=9", INITIAL).tokens;
  assert.equal(ranges.filter((token) => token.scopes.includes("constant.numeric.integer.dodo")).length, 4);
  assert.equal(ranges.filter((token) => token.scopes.includes("keyword.operator.dodo")).length, 2);
  hasScope("value <<= 2", "<<=", "keyword.operator.dodo");
});

test("strings, byte literals, and escapes follow the compiler lexer", () => {
  hasScope('"fn // text"', "fn", "string.quoted.double.dodo");
  hasScope('b"bytes"', "bytes", "string.quoted.double.byte.dodo");
  hasScope("b'X'", "X", "string.quoted.single.byte.dodo");
  hasScope('"\\u{1f600}"', "\\u", "constant.character.escape.dodo");
  hasScope('b"\\xFF\\n"', "\\x", "constant.character.escape.dodo");
  hasScope('b"\\u{41}"', "\\u", "invalid.illegal.escape.dodo");
  hasScope('"\\q"', "\\q", "invalid.illegal.escape.dodo");
  hasScope('// "return 42"', "return", "comment.line.double-slash.dodo");
});

test("unterminated literals and comments do not color the next line", () => {
  for (const source of ['"unfinished', 'b"unfinished', "b'x", "// comment"]) {
    const state = grammar.tokenizeLine(source, INITIAL).ruleStack;
    const next = grammar.tokenizeLine("fn main() {}", state);
    assert.ok(next.tokens[0].scopes.includes("storage.type.function.dodo"), source);
  }
});

test("ordinary identifiers containing keywords remain whole", () => {
  for (const name of ["return_value", "gift", "somewhere", "u128", "format_f32"]) {
    const tokens = grammar.tokenizeLine(name, INITIAL).tokens;
    assert.equal(tokens.length, 1, name);
    assert.deepEqual(tokens[0].scopes, ["source.dodo", "variable.other.dodo"]);
  }
});

test("language configuration uses Dodo comments and usable indentation patterns", () => {
  const config = JSON.parse(fs.readFileSync(path.join(root, "language-configuration.json"), "utf8"));
  assert.deepEqual(config.comments, { lineComment: "//" });
  const increase = new RegExp(config.indentationRules.increaseIndentPattern);
  const decrease = new RegExp(config.indentationRules.decreaseIndentPattern);
  assert.ok(increase.test("fn main() { // entry"));
  assert.ok(!increase.test("// fn main() {"));
  assert.ok(decrease.test("    } else {"));
});

test("all repository examples tokenize without leaving an open string or comment", () => {
  const examples = path.resolve(root, "../../examples");
  for (const file of fs.readdirSync(examples, { recursive: true }).filter((file) => file.endsWith(".dodo"))) {
    let state = INITIAL;
    for (const line of fs.readFileSync(path.join(examples, file), "utf8").split(/\r?\n/)) {
      state = grammar.tokenizeLine(line, state).ruleStack;
    }
    const after = grammar.tokenizeLine("fn after() {}", state);
    assert.ok(after.tokens[0].scopes.includes("storage.type.function.dodo"), file);
  }
});
