import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Script, runInNewContext } from 'node:vm';

const folder=join(dirname(fileURLToPath(import.meta.url)),'..');
const source=readFileSync(join(folder,'desktop-mode.js'),'utf8');
const flush=async()=>{for(let i=0;i<12;i++)await Promise.resolve()};

function harness(native=true){
  const elements=Object.fromEntries(['desktopModeTry','desktopModeKeep',
    'desktopModeCancel','desktopModeStatus'].map(id=>{
    const handlers={};
    return [id,{disabled:false,textContent:'',handlers,
      addEventListener(type,fn){handlers[type]=fn},
      click(){this.handlers.click?.()}
    }];
  }));
  let mode='normal';
  const calls=[],listeners={},timer={current:null,deleted:0};
  const context={
    document:{
      hidden:false,
      getElementById:id=>elements[id]||null,
      addEventListener:(type,fn)=>{listeners[type]=fn}
    },
    window:{
      __TAURI__:native?{core:{invoke:async command=>{
        calls.push(command);
        if(command==='desktop_mode_status')return {mode,trialSeconds:20,genuineDesktopHost:false};
        if(command==='desktop_mode_try'){mode='trial';return {mode}};
        if(command==='desktop_mode_keep'){if(mode!=='trial')throw Error('no trial');mode='bottom';return {mode}};
        if(command==='desktop_mode_cancel'){mode='normal';return {mode}};
        throw Error('unknown command');
      }}}:undefined,
      addEventListener:(type,fn)=>{listeners[type]=fn}
    },
    setInterval(fn,ms){timer.current={fn,ms};return 1},
    clearInterval(id){timer.current=null;timer.deleted++}
  };
  runInNewContext(source,context);
  return {context,elements,calls,listeners,timer,
    adapter:context.window.IslandDesktopMode,
    setNativeMode(next){mode=next}};
}

test('desktop trial source parses and browser mode cannot invoke Win32',()=>{
  assert.doesNotThrow(()=>new Script(source));
  const app=harness(false);
  assert.equal(app.elements.desktopModeTry.disabled,true);
  assert.equal(app.elements.desktopModeKeep.disabled,true);
  assert.equal(app.calls.length,0);
});

test('native mode starts safe and only begins on explicit click',async()=>{
  const app=harness();
  await flush();
  assert.equal(app.adapter.mode,'normal');
  assert.deepEqual(app.calls,['desktop_mode_status']);
  app.elements.desktopModeTry.click();
  await flush();
  assert.equal(app.adapter.mode,'trial');
  assert.equal(app.timer.current.ms,1000);
  assert.equal(app.elements.desktopModeKeep.disabled,false);
});

test('confirmation keeps lower layer for current run, cancel restores normal',async()=>{
  const app=harness();
  await flush();
  app.elements.desktopModeTry.click();await flush();
  app.elements.desktopModeKeep.click();await flush();
  assert.equal(app.adapter.mode,'bottom');
  assert.equal(app.timer.current,null);
  assert.equal(app.elements.desktopModeCancel.disabled,false);
  app.elements.desktopModeCancel.click();await flush();
  assert.equal(app.adapter.mode,'normal');
  assert.equal(app.elements.desktopModeKeep.disabled,true);
  assert.deepEqual(app.calls.slice(1),[
    'desktop_mode_try','desktop_mode_keep','desktop_mode_cancel'
  ]);
});

test('native auto-revert is reflected at the twentieth countdown tick',async()=>{
  const app=harness();
  await flush();
  app.elements.desktopModeTry.click();await flush();
  const tick=app.timer.current.fn;
  for(let i=0;i<19;i++)tick();
  assert.equal(app.adapter.mode,'trial');
  app.setNativeMode('normal');
  tick();await flush();
  assert.equal(app.adapter.mode,'normal');
  assert.equal(app.elements.desktopModeKeep.disabled,true);
});

test('desktop-only control does not reference Explorer WorkerW or Progman',()=>{
  assert.doesNotMatch(source,/SetParent|FindWindowW|SendMessageW/);
  assert.match(source,/desktop_mode_cancel/);
});
