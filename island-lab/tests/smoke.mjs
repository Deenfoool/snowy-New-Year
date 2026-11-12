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

test('online lyrics selector rejects mismatched and unsynced results', () => {
  const start = js.indexOf('function chooseSyncedRecord(');
  const end = js.indexOf('let lyricsSearchRunning=', start);
  assert.ok(start >= 0 && end > start);
  const select = runInNewContext(js.slice(start, end) + '\nchooseSyncedRecord', {});
  const entries = [
    { trackName: 'Other Song', artistName: 'My Artist', duration: 105, syncedLyrics: '[00:01]Wrong' },
    { trackName: 'My Song', artistName: 'My Artist', duration: 190, syncedLyrics: '[00:01]Too long' },
    { trackName: 'My Song', artistName: 'My Artist', duration: 104, syncedLyrics: '[00:01]Matched' },
    { trackName: 'My Song', artistName: 'My Artist', duration: 105, syncedLyrics: null }
  ];
  assert.equal(select(entries, 'My Song', 'My Artist', 105).duration, 104);
  assert.equal(select(entries, 'Unknown', 'My Artist', 105), null);
  assert.equal(select([{ trackName: 'My Song', artistName: 'My Artist', instrumental: true, syncedLyrics: '[00:01]No' }], 'My Song', 'My Artist', 105), null);
});

test('screenshot crop handles reverse drag, clamping and empty selection', () => {
  const start = js.indexOf('function cropRectFromPoints(');
  const end = js.indexOf('function updateCropOutline(', start);
  assert.ok(start >= 0 && end > start);
  const crop = runInNewContext(js.slice(start, end) + '\ncropRectFromPoints', {});
  const a = crop({x: 40,y: 30},{x: 10,y: 5},100,90);
  assert.deepEqual(JSON.parse(JSON.stringify(a)), {x:10,y:5,w:30,h:25});
  const b = crop({x:-10,y:-7},{x:200,y:300},100,90);
  assert.deepEqual(JSON.parse(JSON.stringify(b)), {x:0,y:0,w:100,h:90});
  assert.equal(crop({x:3,y:3},{x:4,y:4},100,90), null);
});

test('online lyrics and crop controls are represented in the UI', () => {
  for (const id of ['findLyrics', 'lyricsTrack', 'lyricsArtist', 'lyricsOffset', 'lyricsEarlier', 'lyricsLater', 'cropOutline', 'removeCrop']) {
    assert.match(html, new RegExp('id="' + id + '"'));
  }
  assert.match(html, /data-tool="crop"/);
  assert.match(js, /Lrclib-Client/);
});
