import test from 'node:test';
import assert from 'node:assert/strict';
import { getPreviewAsset, createPreviewServer } from '../scripts/preview.mjs';

test('preview accepts only canonical Island Lab frontend assets', () => {
  assert.equal(getPreviewAsset('/')[0],'index.html');
  assert.equal(getPreviewAsset('/motion.js?version=1')[0],'motion.js');
  assert.equal(getPreviewAsset('/styles.css')[1],'text/css; charset=utf-8');
  assert.equal(getPreviewAsset('/private.txt'),null);
  assert.equal(getPreviewAsset('/native/src-tauri/src/main.rs'),null);
  assert.equal(getPreviewAsset('/../../.git/config'),null);
});

test('local browser preview serves updated UI without exposing repository files', async () => {
  const server=createPreviewServer();
  await new Promise((resolve,reject)=>{
    server.once('error',reject);
    server.listen(0,'127.0.0.1',resolve)
  });
  const address=server.address();
  const base='http://127.0.0.1:'+address.port;
  try {
    const page=await fetch(base+'/');
    assert.equal(page.status,200);
    assert.match(page.headers.get('content-type'),/text\/html/);
    const html=await page.text();
    assert.match(html,/id="island"/);
    assert.match(html,/src="\.\/motion\.js"/);
    assert.match(html,/id="motionStyle"/);
    const motion=await fetch(base+'/motion.js');
    assert.equal(motion.status,200);
    assert.match(await motion.text(),/function spring\(/);
    const file=await fetch(base+'/native/src-tauri/src/main.rs');
    assert.equal(file.status,404);
    const badMethod=await fetch(base+'/',{method:'POST'});
    assert.equal(badMethod.status,405);
  } finally {
    await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()))
  }
});
