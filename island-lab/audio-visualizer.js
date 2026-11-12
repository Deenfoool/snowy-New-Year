/* WASAPI visualizer for the native Windows host.
 * Deliberately separate from Media Sessions metadata and Web Audio demo.
 * Starts only after the user switches it on. No raw sound leaves Rust.
 */
(function(){
  'use strict';
  const tauri=window.__TAURI__;
  const supported=Boolean(tauri?.core?.invoke&&tauri?.event?.listen);
  let active=false;
  let pending=null;
  let generation=0;
  let unsubscribers=[];
  let lastFrame={bands:Array(16).fill(0),rms:0};
  let lastAt=0;

  function signal(type,detail){
    window.dispatchEvent(new CustomEvent(type,{detail}))
  }
  function cleanListeners(){
    for(const unlisten of unsubscribers){try{unlisten()}catch{}}
    unsubscribers=[];
  }
  function clear(){
    lastFrame={bands:Array(16).fill(0),rms:0};
    lastAt=0;
    signal('island:audio-levels',{frame:lastFrame,active:false});
  }
  async function start(){
    if(!supported)return false;
    if(active)return true;
    if(pending)return pending;
    const epoch=++generation;
    pending=(async()=>{
      try{
        const removeData=await tauri.event.listen('island:audio-spectrum',event=>{
          if(epoch!==generation||!active)return;
          const payload=event.payload;
          if(!payload||!Array.isArray(payload.bands)||payload.bands.length!==16)return;
          lastFrame={
            bands:payload.bands.map(v=>Number.isFinite(v)?Math.min(1,Math.max(0,v)):0),
            rms:Number.isFinite(payload.rms)?Math.min(1,Math.max(0,payload.rms)):0
          };
          lastAt=performance.now();
          signal('island:audio-levels',{frame:lastFrame,active:true})
        });
        if(epoch!==generation){removeData();return false}
        unsubscribers.push(removeData);
        const removeError=await tauri.event.listen('island:audio-error',event=>{
          if(epoch!==generation)return;
          signal('island:audio-capture-error',{error:String(event.payload||'WASAPI unavailable')});
          void stop();
        });
        if(epoch!==generation){removeError();cleanListeners();return false}
        unsubscribers.push(removeError);
        const ok=await tauri.core.invoke('audio_capture_start');
        if(epoch!==generation){
          await tauri.core.invoke('audio_capture_stop');
          return false;
        }
        if(ok!==true)throw new Error('WASAPI capture was not started');
        active=true;
        signal('island:audio-capture-state',{active:true});
        return true;
      }catch(error){
        if(epoch===generation){
          active=false;cleanListeners();clear();
          signal('island:audio-capture-error',{error:String(error)})
        }
        return false;
      }
    })().finally(()=>{pending=null});
    return pending;
  }
  async function stop(){
    // Invalidate any in-flight start immediately. A slow listener or IPC
    // handshake must never re-enable capture after the user turns it off.
    const wasRunning=active||Boolean(pending);
    generation++;
    active=false;cleanListeners();clear();
    signal('island:audio-capture-state',{active:false});
    if(!supported||!wasRunning)return true;
    try{
      // Wait for an in-flight native start to finish before the final stop.
      // The start callback also observes generation and cleans up its own
      // late result; the extra stop makes the ordering unambiguous.
      if(pending)await pending;
      await tauri.core.invoke('audio_capture_stop');
      return true;
    }catch(error){
      signal('island:audio-capture-error',{error:String(error)});
      return false
    }
  }
  document.addEventListener('visibilitychange',()=>{
    // Hidden or minimized UI must not continue capturing system playback.
    if(document.hidden&&(active||pending))void stop()
  });
  window.addEventListener('pagehide',()=>{
    if(active||pending)void stop()
  },{once:true});
  window.IslandAudioCapture={
    supported,start,stop,
    get active(){return active},
    get starting(){return Boolean(pending)},
    get frame(){return lastFrame},
    get lastAt(){return lastAt}
  };
})();
