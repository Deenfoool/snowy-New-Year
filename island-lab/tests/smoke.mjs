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

test('native window shell uses independent Tauri project and same frontend', () => {
  const config = JSON.parse(readFileSync(join(dir, 'native', 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const manifest = readFileSync(join(dir, 'native', 'src-tauri', 'Cargo.toml'), 'utf8');
  const capability = JSON.parse(readFileSync(join(dir, 'native', 'src-tauri', 'capabilities', 'default.json'), 'utf8'));
  assert.equal(config.build.frontendDist, '../dist');
  assert.equal(config.app.windows[0].transparent, true);
  assert.equal(config.app.windows[0].decorations, false);
  assert.equal(config.app.windows[0].alwaysOnTop, false);
  assert.equal(config.bundle.active, false);
  assert.match(manifest, /name = "island-lab-native"/);
  assert.ok(capability.permissions.includes('core:window:allow-start-dragging'));
  assert.match(js, /set_island_expanded/);
  assert.match(css, /html\.native-shell/);
});

test('notes autosave does not use a singular selector as a collection', () => {
  assert.doesNotMatch(js, /\$\('\.note-item'\)\.forEach/);
  assert.match(js, /\$\$\('\.note-item'\)\.forEach/);
});

test('animation loops are gated by visibility and effect settings', () => {
  assert.match(js, /function updateSnowLoop\(/);
  assert.match(js, /document\.hidden/);
  assert.match(js, /function startWaveLoop\(/);
});

test('native security policy explicitly permits IPC but not arbitrary remote hosts', () => {
  const config = JSON.parse(readFileSync(join(dir, 'native', 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const csp = config.app.security.csp;
  assert.match(csp, /connect-src '[^']+' ipc: http:\/\/ipc\.localhost/);
  assert.match(csp, /https:\/\/lrclib\.net/);
  assert.doesNotMatch(csp, /\* https?:/);
});

test('native window exposes a quit command and keeps dragged capsule anchored', () => {
  const rust = readFileSync(join(dir, 'native', 'src-tauri', 'src', 'main.rs'), 'utf8');
  assert.match(rust, /fn quit_island_lab\(/);
  assert.match(rust, /fn set_island_expanded\(/);
  assert.match(rust, /old_position\.x/);
  assert.match(rust, /clamp_inside_screen\(/);
  assert.match(js, /quit_island_lab/);
});

test('PC diagnostics are present and avoid sensitive user content', () => {
  for (const id of ['runDiagnostics','copyDiagnostics','quitNative','diagnosticsOutput']) {
    assert.match(html, new RegExp('id="' + id + '"'));
  }
  const start=js.indexOf('function diagnosticsText()');
  const end=js.indexOf("$('#runDiagnostics')",start);
  assert.ok(start>=0&&end>start);
  const body=js.slice(start,end);
  assert.doesNotMatch(body, /notes\[|noteBody|imageCanvas\.toDataURL|weatherCity|trackName/);
  assert.match(body, /Ошибки JavaScript/);
});

test('LRCLIB throttling guards repeated requests', () => {
  assert.match(js, /lyricsRetryUntil/);
  assert.match(js, /response\.headers\.get\('Retry-After'\)/);
  assert.match(js, /Date\.now\(\)<lyricsRetryUntil/);
});

test('mini player supports a real multi-track local queue', () => {
  assert.match(html, /id="musicFile"[^>]*multiple/);
  assert.match(html, /id="queueCount"/);
  assert.match(js, /async function useFileTrack\(/);
  assert.match(js, /music\.files\.length>1/);
  assert.match(js, /music\.fileIndex\+1/);
  assert.match(js, /URL\.revokeObjectURL/);
});

test('local filename parser derives artist and title without hardcoding', () => {
  const start=js.indexOf('function parseLocalTrack(');
  const end=js.indexOf('async function useFileTrack(',start);
  assert.ok(start>=0&&end>start);
  const parse=runInNewContext(js.slice(start,end)+'\nparseLocalTrack',{});
  assert.deepEqual(JSON.parse(JSON.stringify(parse('Band - Title.mp3'))),{artist:'Band',title:'Title'});
  assert.deepEqual(JSON.parse(JSON.stringify(parse('Untitled.wav'))),{artist:'',title:'Untitled'});
});

test('native player source is packaged with an explicit Windows-only switch', () => {
  const bridge=readFileSync(join(dir,'system-media.js'),'utf8');
  const stage=readFileSync(join(dir,'native','scripts','prepare.mjs'),'utf8');
  const nativeMain=readFileSync(join(dir,'native','src-tauri','src','main.rs'),'utf8');
  const media=readFileSync(join(dir,'native','src-tauri','src','media.rs'),'utf8');
  assert.doesNotThrow(()=>new Script(bridge,{filename:'system-media.js'}));
  assert.match(html,/src="\.\/system-media\.js"/);
  assert.match(html,/id="systemPlayerToggle"/);
  assert.match(css,/html\.native-shell \.native-only/);
  assert.match(stage,/system-media\.js/);
  assert.match(nativeMain,/media::system_media_snapshot/);
  assert.match(nativeMain,/media::system_media_action/);
  assert.match(media,/GlobalSystemMediaTransportControlsSessionManager/);
  assert.match(media,/TryTogglePlayPauseAsync/);
  assert.match(js,/function disableSystemMusic\(/);
});

test('WASAPI capture is gated by a native-only explicit consent toggle', () => {
  const visual=readFileSync(join(dir,'audio-visualizer.js'),'utf8');
  const rust=readFileSync(join(dir,'native','src-tauri','src','audio.rs'),'utf8');
  const nativeMain=readFileSync(join(dir,'native','src-tauri','src','main.rs'),'utf8');
  const stage=readFileSync(join(dir,'native','scripts','prepare.mjs'),'utf8');
  const manifest=readFileSync(join(dir,'native','src-tauri','Cargo.toml'),'utf8');
  assert.doesNotThrow(()=>new Script(visual,{filename:'audio-visualizer.js'}));
  assert.match(html,/id="systemWaveToggle"/);
  assert.match(html,/Звуковые волны|Волны из звука Windows/);
  assert.match(html,/src="\.\/audio-visualizer\.js"/);
  assert.match(stage,/audio-visualizer\.js/);
  assert.match(js,/const audioCapture=window\.IslandAudioCapture/);
  assert.match(js,/function nativeSystemAudioActive\(/);
  assert.match(js,/audioCapture\.start\(\)/);
  assert.match(nativeMain,/audio::audio_capture_start/);
  assert.match(nativeMain,/audio::audio_capture_stop/);
  assert.match(manifest,/wasapi = "0\.27"/);
  assert.match(rust,/Direction::Render/);
  assert.match(rust,/Direction::Capture/);
  assert.match(rust,/fn analyze\(/);
  assert.match(rust,/fn silence_produces_zero_spectrum\(/);
});

test('native desktop layering has a guarded 20-second rollback, not WorkerW injection',()=>{
  const overlay=readFileSync(join(dir,'desktop-mode.js'),'utf8');
  const rust=readFileSync(join(dir,'native','src-tauri','src','desktop.rs'),'utf8');
  const main=readFileSync(join(dir,'native','src-tauri','src','main.rs'),'utf8');
  const staging=readFileSync(join(dir,'native','scripts','prepare.mjs'),'utf8');
  assert.doesNotThrow(()=>new Script(overlay,{filename:'desktop-mode.js'}));
  for(const id of ['desktopModeStatus','desktopModeTry','desktopModeKeep','desktopModeCancel'])
    assert.match(html,new RegExp('id="'+id+'"'));
  assert.match(html,/src="\.\/desktop-mode\.js"/);
  assert.match(staging,/desktop-mode\.js/);
  assert.match(main,/desktop::desktop_mode_try/);
  assert.match(main,/desktop::desktop_mode_cancel/);
  assert.match(rust,/TRIAL_SECONDS: u64 = 20/);
  assert.match(rust,/set_always_on_bottom\(true\)/);
  assert.match(rust,/set_always_on_bottom\(false\)/);
  assert.match(rust,/run_on_main_thread/);
  assert.match(rust,/Mutex<Inner>/);
  assert.match(rust,/genuine_desktop_host: false/);
  assert.doesNotMatch(rust,/SetParent|FindWindowW|SendMessageW/);
});
