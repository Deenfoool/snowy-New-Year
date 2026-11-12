import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Script, runInNewContext } from 'node:vm';

const here=dirname(fileURLToPath(import.meta.url));
const code=readFileSync(join(here,'..','audio-visualizer.js'),'utf8');
const tick=async()=>{for(let i=0;i<8;i++)await Promise.resolve()};

function mock(native=true){
  const events=[],commands=[],listeners=new Map(),unsubs=[];
  const domListeners={};
  const dom={hidden:false,addEventListener:(name,fn)=>{domListeners[name]=fn}};
  const target={
    __TAURI__:native?{
      core:{invoke:async(command,args)=>{commands.push({command,args});return true}},
      event:{listen:async(name,handler)=>{
        listeners.set(name,handler);
        return ()=>{listeners.delete(name);unsubs.push(name)}
      }}
    }:undefined,
    dispatchEvent:event=>events.push(event),
    addEventListener:(name,fn)=>{domListeners['window:'+name]=fn}
  };
  const context={
    window:target,document:dom,performance:{now:()=>1200},
    CustomEvent:class{constructor(type,data){this.type=type;this.detail=data.detail}}
  };
  runInNewContext(code,context);
  return {manager:target.IslandAudioCapture,events,commands,listeners,unsubs,dom,domListeners}
}

test('WASAPI bridge parses without syntax errors',()=>{
  assert.doesNotThrow(()=>new Script(code));
});

test('Browser preview never starts WASAPI or Win32 capture',async()=>{
  const m=mock(false);
  assert.equal(m.manager.supported,false);
  assert.equal(await m.manager.start(),false);
  assert.deepEqual(m.commands,[]);
});

test('No audio is captured until explicit opt-in; only 16 levels are returned',async()=>{
  const m=mock();
  assert.equal(m.commands.length,0);
  assert.equal(await m.manager.start(),true);
  assert.equal(m.manager.active,true);
  assert.equal(m.commands[0].command,'audio_capture_start');
  m.listeners.get('island:audio-spectrum')({
    payload:{bands:Array(16).fill(.4),rms:.31}
  });
  assert.equal(m.manager.frame.bands.length,16);
  assert.equal(m.manager.frame.rms,.31);
  assert.equal(m.manager.lastAt,1200);
  assert.equal(m.events.some(e=>e.type==='island:audio-levels'),true);
  await m.manager.stop();
  assert.equal(m.manager.active,false);
  assert.equal(m.commands.at(-1).command,'audio_capture_stop');
  assert.equal(m.unsubs.length,2);
  assert.equal(m.manager.frame.rms,0);
});

test('Malformed frames are ignored and values are clamped',async()=>{
  const m=mock();
  await m.manager.start();
  m.listeners.get('island:audio-spectrum')({payload:{bands:[1,2],rms:10}});
  assert.equal(m.manager.lastAt,0);
  m.listeners.get('island:audio-spectrum')({
    payload:{bands:[NaN,-2,...Array(13).fill(.2),5],rms:5}
  });
  assert.equal(m.manager.frame.bands[0],0);
  assert.equal(m.manager.frame.bands[1],0);
  assert.equal(m.manager.frame.bands[15],1);
  assert.equal(m.manager.frame.rms,1);
  await m.manager.stop()
});

test('Minimizing or hiding the native window stops capture',async()=>{
  const m=mock();
  await m.manager.start();
  m.dom.hidden=true;
  m.domListeners.visibilitychange();
  await tick();
  assert.equal(m.manager.active,false);
  assert.equal(m.commands.at(-1).command,'audio_capture_stop');
});

test('WASAPI error stops capture and publishes a status',async()=>{
  const m=mock();
  await m.manager.start();
  m.listeners.get('island:audio-error')({payload:'no output device'});
  await tick();
  assert.equal(m.manager.active,false);
  assert.equal(m.commands.at(-1).command,'audio_capture_stop');
  assert.equal(m.events.some(e=>e.type==='island:audio-capture-error'),true);
});
