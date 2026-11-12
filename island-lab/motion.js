/* Island Lab motion primitives: one design language for the web and WebView2.
 * No framework, no background loop and no access to privileged Windows APIs.
 */
(function(){
  'use strict';
  const supported=typeof window.matchMedia==='function';
  const prefersReduced=()=>supported&&window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const presets={
    fluid:{morph:560,reveal:400,tab:390,lyric:520,press:140},
    quick:{morph:390,reveal:270,tab:270,lyric:340,press:110},
    calm:{morph:700,reveal:520,tab:450,lyric:590,press:170}
  };
  let preset='fluid';
  function motionAllowed(userReduced=false){return !userReduced&&!prefersReduced()}
  function setPreset(next){
    preset=Object.prototype.hasOwnProperty.call(presets,next)?next:'fluid';
    document.documentElement.dataset.motion=preset;
    return preset
  }
  function timings(){return presets[preset]}
  function smooth(current,target,seconds,response=12){
    if(!Number.isFinite(current)||!Number.isFinite(target))return target;
    if(!(seconds>0))return current;
    const dt=Math.min(.06,seconds);
    return current+(target-current)*(1-Math.exp(-Math.max(.1,response)*dt))
  }
  // Stable analytic damped spring. Calling it with irregular animation-frame
  // deltas does not change the result materially or cause numeric blow-ups.
  function spring(x,v,target,seconds,frequency=3.5,damping=.82){
    if(!Number.isFinite(target))return {x:0,v:0};
    if(!Number.isFinite(x)||!Number.isFinite(v))return {x:target,v:0};
    const t=Math.min(.09,Math.max(0,seconds));
    if(!t)return{x,v};
    const omega=2*Math.PI*Math.max(.1,frequency);
    const zeta=Math.min(.999,Math.max(.05,damping));
    const wd=omega*Math.sqrt(1-zeta*zeta);
    const e=Math.exp(-zeta*omega*t);
    const c=Math.cos(wd*t), s=Math.sin(wd*t);
    const y=x-target;
    const k=(v+zeta*omega*y)/wd;
    const displacement=e*(y*c+k*s);
    const velocity=e*((v)*c+(-zeta*omega*k-y*wd)*s);
    return {x:target+displacement,v:velocity}
  }

  // Recreate the optical "reel" without adding markup or changing the
  // semantic order of the three current lyrics. Old lines move upward;
  // new lines rise gently and become focused.
  let lyricGhosts=[];
  function clearLyrics(){
    for(const ghost of lyricGhosts){ghost.getAnimations?.().forEach(a=>a.cancel());ghost.remove()}
    lyricGhosts=[];
  }
  function updateLyrics(container,elements,words,{direction=1,enabled=true}={}){
    if(!container||elements.length!==3||words.length!==3)return;
    const old=elements.map(el=>el.textContent);
    if(old.every((value,i)=>value===words[i]))return;
    for(const el of elements)el.getAnimations?.().forEach(a=>a.cancel());
    clearLyrics();
    const animate=enabled&&motionAllowed()&&typeof elements[0].animate==='function';
    if(!animate){
      elements.forEach((el,i)=>{el.textContent=words[i]});
      return
    }
    const containerBox=container.getBoundingClientRect();
    const clones=[];
    elements.forEach((el,i)=>{
      const rect=el.getBoundingClientRect();
      const ghost=document.createElement('span');
      ghost.className='lyric-motion-ghost '+(i===1?'current':'muted');
      ghost.textContent=old[i];
      Object.assign(ghost.style,{
        position:'absolute',
        width:rect.width+'px',
        left:(rect.left-containerBox.left)+'px',
        top:(rect.top-containerBox.top)+'px',
        pointerEvents:'none'
      });
      container.appendChild(ghost);
      clones.push(ghost);
    });
    lyricGhosts=clones;
    const t=timings().lyric;
    clones.forEach((ghost,i)=>{
      ghost.animate([
        {opacity:i===1?1:.6,transform:'translate3d(0,0,0) scale(1)',filter:'blur(0px)'},
        {opacity:0,transform:'translate3d(0,'+(-direction*24)+'px,0) scale(.965)',filter:'blur(3px)'}
      ],{duration:Math.round(t*.66),easing:'cubic-bezier(.2,.7,.2,1)',fill:'forwards'})
    });
    elements.forEach((el,i)=>{
      el.textContent=words[i];
      const delay=i===1?45:65;
      el.animate([
        {opacity:0,transform:'translate3d(0,'+(direction*22)+'px,0) scale(.98)',filter:'blur(2px)'},
        {opacity:i===1?1:.66,transform:'translate3d(0,0,0) scale(1)',filter:'blur(0px)'}
      ],{duration:t,delay,easing:'cubic-bezier(.18,1,.3,1)',fill:'backwards'})
    });
    const scheduled=clones;
    setTimeout(()=>{
      for(const ghost of scheduled){ghost.getAnimations?.().forEach(a=>a.cancel());ghost.remove()}
      if(lyricGhosts===scheduled)lyricGhosts=[];
    },t+95)
  }

  function onceTransition(element,property,maxMs){
    if(!element)return Promise.resolve();
    if(!motionAllowed()||maxMs<=0)return Promise.resolve();
    return new Promise(resolve=>{
      let ended=false;
      const finish=()=>{
        if(ended)return;
        ended=true;clearTimeout(timer);
        element.removeEventListener('transitionend',onEnd);
        resolve()
      };
      const onEnd=e=>{
        if(e.target===element&&e.propertyName===property)finish()
      };
      element.addEventListener('transitionend',onEnd);
      const timer=setTimeout(finish,maxMs+100)
    })
  }

  window.IslandMotion={
    presets,setPreset,timings,motionAllowed,smooth,spring,
    updateLyrics,clearLyrics,onceTransition,
    get preset(){return preset}
  };
})();
