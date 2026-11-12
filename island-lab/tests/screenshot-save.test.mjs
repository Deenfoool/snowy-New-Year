import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {Script,runInNewContext} from 'node:vm';

const root=join(dirname(fileURLToPath(import.meta.url)),'..');
const app=readFileSync(join(root,'app.js'),'utf8');
const rust=readFileSync(join(root,'native','src-tauri','src','screenshots.rs'),'utf8');
const html=readFileSync(join(root,'index.html'),'utf8');

function harness(native,shouldFail=false){
  const clicks=[],calls=[],toasts=[],urls=[],status={textContent:''};
  const begin=app.indexOf('function downloadPng(');
  const end=app.indexOf('let savingScreenshot=false;',begin);
  assert.ok(begin>=0&&end>begin,'savePngBlob section not found');
  const extracted=app.slice(begin,end);
  const context={
    nativeShell:native,
    nativeApi:{core:{invoke:async(command,args)=>{
      calls.push({command,args});
      if(shouldFail)throw Error('write blocked');
      return 'Pictures/Island Lab/screenshot.png'
    }}},
    FileReader:class{
      readAsDataURL(){
        this.result='data:image/png;base64,cG5n';
        queueMicrotask(()=>this.onload?.())
      }
    },
    document:{createElement(type){
      assert.equal(type,'a');
      return {click(){clicks.push({href:this.href,download:this.download})}}
    }},
    URL:{createObjectURL(blob){urls.push(blob);return 'blob:save'},revokeObjectURL(){}},
    Date,Error,Promise,
    setTimeout(){},
    toast(message){toasts.push(message)},
    console:{warn(){}},
    $:id=>id==='#editorStatus'?status:null
  };
  const save=runInNewContext(extracted+'\nsavePngBlob',context);
  return {save,calls,clicks,toasts,urls,status}
}

test('native screenshot export calls Rust and leaves browser downloads alone',async()=>{
  const h=harness(true);
  const data={type:'image/png'};
  await h.save(data);
  assert.equal(h.calls.length,1);
  assert.equal(h.calls[0].command,'save_screenshot_native');
  assert.equal(h.calls[0].args.pngData,'data:image/png;base64,cG5n');
  assert.equal(h.clicks.length,0);
  assert.match(h.status.textContent,/Pictures\/Island Lab\/screenshot.png/);
});

test('failure of native screenshot save falls back to ordinary browser PNG download',async()=>{
  const h=harness(true,true);
  await h.save({type:'image/png'});
  assert.equal(h.calls.length,1);
  assert.equal(h.clicks.length,1);
  assert.match(h.clicks[0].download,/Island-Lab-\d+\.png/);
  assert.match(h.toasts.at(-1),/скачан браузером/);
});

test('browser version does not invoke Rust and still downloads PNG',async()=>{
  const h=harness(false);
  await h.save({type:'image/png'});
  assert.equal(h.calls.length,0);
  assert.equal(h.clicks.length,1);
  assert.match(h.toasts.at(-1),/через браузер/);
});

test('native screenshot commands and user-facing folder button are wired',()=>{
  assert.match(html,/id="openShotsFolder"/);
  assert.match(html,/class="secondary-action native-only"/);
  assert.match(app,/open_screenshot_folder_native/);
  assert.match(rust,/fn save_screenshot_native\(/);
  assert.match(rust,/fn open_screenshot_folder_native\(/);
  assert.match(rust,/dirs::picture_dir\(\)/);
  assert.match(rust,/create_new\(true\)/);
  assert.match(rust,/MAX_PNG_BYTES/);
});

test('Rust screenshot backend does not accept arbitrary file names from JavaScript',()=>{
  assert.doesNotMatch(rust,/fn save_screenshot_native\(path/);
  assert.match(rust,/fn save_screenshot_native\(png_data: String\)/);
  assert.match(rust,/format!\("Island-Lab-/);
  assert.match(rust,/create_new\(true\)/);
});
