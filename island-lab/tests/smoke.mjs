import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Script, runInNewContext } from 'node:vm';

const dir = join(dirname(fileURLToPath(import.meta.url)), '..');
const html = readFileSync(join(dir, 'index.html'), 'utf8');
const css = readFileSync(join(dir, 'styles.css'), 'utf8');
const js = readFileSync(join(dir, 'app.js'), 'utf8');

test('JavaScript parses without syntax errors', () => {
  assert.doesNotThrow(() => new Script(js, { filename: 'app.js' }));
});

test('all ID selectors in JS refer to a unique HTML ID', () => {
  const ids = [...html.matchAll(/id="([^"]+)"/g)].map(match => match[1]);
  assert.equal(new Set(ids).size, ids.length);
  const selectors = [...js.matchAll(/\$\('#([^']+)'\)/g)].map(match => match[1]);
  for (const id of selectors) assert.ok(ids.includes(id), `Missing element #${id}`);
});

test('tabs correspond to views', () => {
  const tabs = [...html.matchAll(/data-tab="([^"]+)"/g)].map(match => match[1]);
  assert.equal(tabs.length, 5);
  for (const tab of tabs) assert.match(html, new RegExp('id="view-' + tab + '"'));
});

test('web page references locally provided CSS and JS', () => {
  assert.match(html, /href="\.\/styles\.css"/);
  assert.match(html, /src="\.\/app\.js"/);
  assert.match(css, /#waveCanvas/);
});

test('LRC parser supports multiple timestamps and sorts lyrics', () => {
  const line = js.split('\n').find(x => x.startsWith('function parseLRC('));
  assert.ok(line, 'parseLRC should stay extractable as a pure helper');
  const read = runInNewContext(line + '\nparseLRC', {});
  const result = read('[00:04.50]later\n[00:01.25][00:03.00]early');
  assert.equal(result.length, 3);
  assert.deepEqual(Array.from(result, x => Number(x.at.toFixed(2))), [1.25, 3, 4.5]);
  assert.equal(result[0].text, 'early');
});
