import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const grammar = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repository = resolve(grammar, '../..');
const cli = join(grammar, 'node_modules/tree-sitter-cli/cli.js');
const language = resolve(grammar, '../dodo-zed/languages/dodo');

function run(...args) {
  try {
    return execFileSync(process.execPath, [cli, ...args], {
      cwd: grammar,
      encoding: 'utf8',
      maxBuffer: 16 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  } catch (error) {
    process.stderr.write(error.stdout ?? '');
    process.stderr.write(error.stderr ?? '');
    throw new Error(`tree-sitter ${args[0]} failed (exit ${error.status})`);
  }
}

function sources(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? sources(path) : entry.name.endsWith('.dodo') ? [path] : [];
  });
}

const files = ['examples', 'stdlib', 'tests'].flatMap(name => sources(join(repository, name)));
assert.ok(files.length > 0);
run('parse', '--quiet', ...files);
console.log(`Parsed ${files.length} repository Dodo files without errors.`);

const fixture = join(grammar, 'test/highlight/syntax.dodo');
for (const name of readdirSync(language).filter(name => name.endsWith('.scm'))) {
  run('query', '--quiet', join(language, name), fixture);
}
console.log('Compiled and executed every Zed query.');

const outline = run('query', '--captures', join(language, 'outline.scm'), join(grammar, 'test/outline.dodo'));
const names = [...outline.matchAll(/capture: \d+ - name,.*text: `([^`]+)`/g)].map(match => match[1]);
assert.deepEqual(names, ['Buffer', 'view', 'State', 'foreign', 'main']);
// A prototype must end on its own line; a method's outline range includes its body.
const items = [...outline.matchAll(/capture: \d+ - item, start: \((\d+), \d+\), end: \((\d+), \d+\)/g)]
  .map(match => [Number(match[1]), Number(match[2])]);
assert.deepEqual(items, [[2, 9], [5, 8], [11, 11], [14, 14], [17, 20]]);
console.log('Outline names and declaration ranges passed.');
