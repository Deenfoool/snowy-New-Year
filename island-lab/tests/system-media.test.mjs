import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {runInNewContext,Script} from 'node:vm';

const root=join(dirname(fileURLToPath(import.meta.url)),'..');
const script=readFileSync(join(root,'system-media.js'),'utf8');

function prepare(native=true){
  const calls=[],events=[],intervals=[],cleared=[],handlers={};
  const adapter={
    hidden:false,
    addEventListener(type,cb){handlers[type]=cb}
  };
  const window={
    __TAURI__:native?{core:{
      invoke:async(command,args)=>{
        calls.push({command,args});
        if(command==='system_media_snapshot')return {title:'Track',artist:'Artist',playing:true,positionSeconds:14,durationSeconds:120};
        if(command==='system_media_action')return true;
        throw new Error('Unknown native command')
      }
    }}:undefined,
    addEventListener(type,cb){handlers['window:'+type]=cb},
    dispatchEvent(event){events.push(event.detail)}
  };
  const context={
    window,document:adapter,
    CustomEvent:class{constructor(_,options){this.detail=options.detail}},
    setInterval:(callback,time)=>{intervals.push({callback,time});return intervals.length},
    clearInterval:id=>{cleared.push(id)},
    setTimeout:()=>1
  };
  runInNewContext(script,context);
  return {adapter:window.IslandSystemMedia,calls,events,intervals,cleared,handlers,document:adapter};
}
const flush=async()=>{await Promise.resolve();await Promise.resolve();await Promise.resolve();};

test('native bridge JavaScript parses and does not poll at startup',()=>{
  assert.doesNotThrow(()=>new Script(script));
  const mock=prepare(true);
  assert.equal(mock.adapter.available,true);
  assert.equal(mock.calls.length,0);
  assert.equal(mock.intervals.length,0);
});

test('browser mode exposes no native controls and stays idle',async()=>{
  const mock=prepare(false);
  assert.equal(mock.adapter.available,false);
  assert.equal(mock.adapter.enable(),false);
  await mock.adapter.refresh();
  assert.equal(mock.calls.length,0);
});

test('opt-in enable polls status and disables without leaving an active interval',async()=>{
  const mock=prepare(true);
  assert.equal(mock.adapter.enable(),true);
  assert.equal(mock.adapter.enable(),true);
  await flush();
  assert.equal(mock.intervals.length,1);
  assert.equal(mock.intervals[0].time,2000);
  assert.equal(mock.calls[0].command,'system_media_snapshot');
  assert.equal(mock.events.at(-1)?.snapshot?.title,'Track');
  mock.adapter.disable();
  assert.equal(mock.adapter.enabled,false);
  assert.equal(mock.cleared.length,1);
});

test('system playback commands go through the native adapter only while enabled',async()=>{
  const mock=prepare(true);
  assert.equal(await mock.adapter.control('next'),false);
  mock.adapter.enable();
  await flush();
  assert.equal(await mock.adapter.control('next'),true);
  assert.equal(await mock.adapter.control('seek',35.5),true);
  assert.equal(mock.calls.some(call=>call.command==='system_media_action'&&call.args.action==='next'),true);
  assert.equal(mock.calls.some(call=>call.command==='system_media_action'&&call.args.action==='seek'&&call.args.seconds===35.5),true);
  assert.equal(await mock.adapter.control('unsupported'),false);
  mock.adapter.disable();
});

test('no polling in background and previous responses are discarded on disable',async()=>{
  const mock=prepare(true);
  mock.document.hidden=true;
  mock.adapter.enable();
  await flush();
  assert.equal(mock.calls.length,0);
  mock.document.hidden=false;
  mock.handlers.visibilitychange();
  await flush();
  assert.equal(mock.calls.length,1);
  mock.adapter.disable();
});
