/* Island Lab native system player adapter.
 * Browser mode never requests Windows privileges. No polling until opt-in.
 */
(function(){
  'use strict';
  const invoke=window.__TAURI__?.core?.invoke;
  const available=typeof invoke==='function';
  let enabled=false;
  let timer=null;
  let requestInFlight=false;
  let generation=0;
  let last=null;

  function publish(snapshot,error=null){
    last=snapshot;
    window.dispatchEvent(new CustomEvent('island:system-media',{
      detail:{snapshot,error,enabled}
    }));
  }
  async function refresh(){
    if(!enabled||!available||requestInFlight||document.hidden)return;
    requestInFlight=true;
    const epoch=generation;
    try{
      const snapshot=await invoke('system_media_snapshot');
      if(epoch===generation&&enabled)publish(snapshot);
    }catch(e){
      if(epoch===generation&&enabled)publish(null,String(e));
    }finally{
      requestInFlight=false;
    }
  }
  function enable(){
    if(!available)return false;
    if(enabled)return true;
    enabled=true;generation++;
    timer=setInterval(refresh,2000);
    refresh();
    return true;
  }
  function disable(){
    enabled=false;generation++;
    if(timer!==null)clearInterval(timer);
    timer=null;
    last=null;
  }
  async function control(action,seconds){
    if(!enabled||!available)return false;
    if(!['toggle','next','previous','play','pause','seek'].includes(action))return false;
    try{
      const ok=await invoke('system_media_action',{action,seconds:seconds??null});
      // Some players report their new state with a delay.
      setTimeout(refresh,320);
      return ok===true;
    }catch(e){
      publish(last,String(e));
      return false;
    }
  }
  document.addEventListener('visibilitychange',()=>{
    if(enabled&&!document.hidden)refresh()
  });
  window.addEventListener('pagehide',disable,{once:true});
  window.IslandSystemMedia={
    available,enable,disable,control,refresh,
    get enabled(){return enabled},
    get snapshot(){return last}
  };
})();
