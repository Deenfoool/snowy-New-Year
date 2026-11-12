/* Island Lab: reversible desktop layering trial.
 * Never binds directly to WorkerW or Progman and never stores this preference.
 */
(function () {
  'use strict';
  const tauri=window.__TAURI__?.core;
  const native=typeof tauri?.invoke==='function';
  const byId=id=>document.getElementById(id);
  const start=byId('desktopModeTry');
  const keep=byId('desktopModeKeep');
  const cancel=byId('desktopModeCancel');
  const output=byId('desktopModeStatus');
  if(!start||!keep||!cancel||!output)return;

  let mode='normal',busy=false,secondsLeft=0,ticker=null;
  let lastError='';
  const canUse=native;

  function stopTicker(){
    if(ticker!==null)clearInterval(ticker);
    ticker=null
  }
  function render(){
    const state=mode==='trial'
      ? 'Проба нижнего слоя · возврат через '+Math.max(0,secondsLeft)+' с'
      : mode==='bottom'
        ? 'Нижний слой до перезапуска · можно вернуть вручную'
        : 'Обычное окно · безопасный режим';
    output.textContent=lastError?'Ошибка: '+lastError:state;
    start.disabled=!canUse||busy||mode==='trial';
    keep.disabled=!canUse||busy||mode!=='trial';
    cancel.disabled=!canUse||busy||mode==='normal';
  }
  async function status(){
    if(!canUse)return {mode:'normal'};
    try{
      const latest=await tauri.invoke('desktop_mode_status');
      if(!latest)return null;
      mode=latest.mode||'normal';
      if(mode!=='trial')stopTicker();
      lastError='';render();
      return latest;
    }catch(error){
      lastError=String(error).slice(0,130);render();
      return null;
    }
  }
  function countDown(){
    stopTicker();secondsLeft=20;
    ticker=setInterval(async()=>{
      secondsLeft=Math.max(0,secondsLeft-1);
      if(secondsLeft===0){stopTicker();await status()}
      else render();
    },1000);
  }
  async function invoke(action){
    if(!canUse||busy)return;
    busy=true;lastError='';render();
    try{
      const result=await tauri.invoke(action);
      mode=result.mode;
      if(mode==='trial')countDown();
      else stopTicker();
    }catch(error){
      lastError=String(error).slice(0,130);
      // A failed action must not leave the local UI claiming success.
      await status();
      if(!lastError)lastError='Не удалось изменить режим окна';
    }finally{
      busy=false;render();
    }
  }
  start.addEventListener('click',()=>invoke('desktop_mode_try'));
  keep.addEventListener('click',()=>invoke('desktop_mode_keep'));
  cancel.addEventListener('click',()=>invoke('desktop_mode_cancel'));
  document.addEventListener('visibilitychange',()=>{
    if(!document.hidden&&canUse)void status()
  });
  window.addEventListener('pagehide',stopTicker,{once:true});
  if(canUse)void status();
  else render();
  window.IslandDesktopMode={get mode(){return mode},get busy(){return busy},refresh:status};
})();
