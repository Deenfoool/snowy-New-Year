/* Island Lab: self-contained browser experiment, independent from Snowy New Year. */
'use strict';
const $=s=>document.querySelector(s), $$=s=>[...document.querySelectorAll(s)];
// The same frontend is used in a browser and in the optional Tauri/WebView2
// host. Browser mode never requests native privileges.
const nativeApi=window.__TAURI__;
const nativeShell=Boolean(nativeApi?.core?.invoke);
let nativeBridgeStatus=nativeShell?'Ожидает ответа':'Браузерная версия';
let runtimeErrorCount=0;
window.addEventListener('error',()=>{runtimeErrorCount++});
window.addEventListener('unhandledrejection',()=>{runtimeErrorCount++});
document.documentElement.classList.toggle('native-shell',nativeShell);
let nativeResizeQueue=Promise.resolve();
function resizeNativeWindow(expanded){
  if(!nativeShell)return Promise.resolve(true);
  // Serialize Win32 window geometry changes. A slow completion from a prior
  // expand request must never overwrite a newer collapse request.
  const apply=()=>nativeApi.core.invoke('set_island_expanded',{expanded})
    .then(()=>{nativeBridgeStatus='OK';return true})
    .catch(error=>{
      nativeBridgeStatus='Ошибка IPC';
      console.warn('Island Lab: native window resize failed',error);
      return false
    });
  nativeResizeQueue=nativeResizeQueue.then(apply,apply);
  return nativeResizeQueue
}
const KEY='island-lab-v1', NOTES='island-lab-notes-v1';
const defaults={accent:'#8dd9f6',wavePower:65,waveEnabled:true,reducedMotion:false,backgroundSnow:true,snow:true,caps:true,snowmen:true,snowIntensity:42,weatherCity:'',lyricsOffset:0,motionStyle:'fluid',offset:null};
const options=(()=>{try{return {...defaults,...JSON.parse(localStorage.getItem(KEY)||'{}')}}catch{return {...defaults}}})();
function persist(){try{localStorage.setItem(KEY,JSON.stringify(options))}catch{}}
function toast(message){const el=$('#toast');el.textContent=message;el.classList.add('visible');clearTimeout(toast.timer);toast.timer=setTimeout(()=>el.classList.remove('visible'),3400)}
function rgb(hex){const h=hex.replace('#','');const v=parseInt(h,16);return [(v>>16)&255,(v>>8)&255,v&255].join(',')}
function applyOptions(){document.documentElement.style.setProperty('--accent',options.accent);document.documentElement.style.setProperty('--accent-rgb',rgb(options.accent));$('#accentColor').value=options.accent;$('#wavePower').value=options.wavePower;$('#waveValue').textContent=options.wavePower+'%';$('#waveToggle').checked=options.waveEnabled;$('#reducedMotion').checked=options.reducedMotion;$('#backgroundSnow').checked=options.backgroundSnow;$('#snowToggle').checked=options.snow;$('#capsToggle').checked=options.caps;$('#snowmenToggle').checked=options.snowmen;$('#snowIntensity').value=options.snowIntensity;$('#snowPercent').textContent=options.snowIntensity+'%';$('#weatherInput').value=options.weatherCity||'';
  const selectedMotion=window.IslandMotion?.setPreset(options.motionStyle)||options.motionStyle;
  document.documentElement.classList.toggle('motion-reduced',Boolean(options.reducedMotion));
  $('#motionStyle').value=selectedMotion;
  $('#lyricsOffset').textContent=(options.lyricsOffset>0?'+':'')+options.lyricsOffset.toFixed(1)+' с';if(options.offset&&!nativeShell)placeIsland(options.offset.left,options.offset.top)}
function setOption(k,v){
  options[k]=v;persist();applyOptions();
  if(['waveEnabled','reducedMotion','wavePower'].includes(k)){drawWave();startWaveLoop()}
  if(['snow','backgroundSnow','reducedMotion','snowIntensity'].includes(k))updateSnowLoop();
  if(k==='motionStyle'||k==='reducedMotion')refreshIslandHeight()
}
const shell=$('#shell'),island=$('#island'),panel=$('#expandedPanel');
const motion=window.IslandMotion;
function animateLabel(selector,value){
  const element=$(selector);
  if(motion)motion.animateValue(element,value,{enabled:expanded&&!options.reducedMotion});
  else if(element)element.textContent=String(value??'')
}
function animateCoverChange(){
  if(!expanded||tab!=='home'||!motion?.motionAllowed(options.reducedMotion))return;
  const artwork=$('.cover');
  if(!artwork||typeof artwork.animate!=='function')return;
  artwork.getAnimations?.().forEach(animation=>animation.cancel());
  artwork.animate([
    {opacity:.65,transform:'translate3d(0,5px,0) scale(.945)'},
    {opacity:1,transform:'translate3d(0,0,0) scale(1)'}
  ],{
    duration:motion.timings().reveal,
    easing:'cubic-bezier(.18,1,.3,1)'
  })
}
let expanded=false,tab='home',morphSequence=0,morphTimer=0;
const nav=$('.tabs');
const collapsedBar=$('#collapsedBar');
let tabRequest=0,tabTransition=null;
const navIndicator=document.createElement('span');
navIndicator.className='tabs-indicator';
navIndicator.setAttribute('aria-hidden','true');
nav.prepend(navIndicator);

function syncTabIndicator(){
  const active=$('.tab.active');
  if(!active)return;
  nav.style.setProperty('--tab-indicator-x',active.offsetLeft+'px');
  nav.style.setProperty('--tab-indicator-width',active.offsetWidth+'px');
  nav.style.setProperty('--tab-indicator-height',active.offsetHeight+'px')
}
function refreshIslandHeight(){
  if(!expanded||island.classList.contains('collapsed'))return;
  // Measure the end-state content width, not the transient narrow capsule.
  const finalWidth=Math.max(280,shell.clientWidth-2);
  const before=panel.style.width;
  panel.style.width=finalWidth+'px';
  const measured=panel.getBoundingClientRect().height;
  panel.style.width=before;
  island.style.setProperty('--island-height',
    Math.ceil(Math.max(49,measured+2))+'px');
}
function commitExpanded(next){
  const token=++morphSequence;
  expanded=next;
  clearTimeout(morphTimer);
  island.classList.add('is-morphing');
  island.classList.toggle('collapsed',!next);
  // Make only the currently visible layout keyboard-reachable. Preserve
  // focus when its old container becomes inert, especially for Escape.
  const focused=document.activeElement;
  const cameFromMini=next&&collapsedBar.contains(focused);
  const cameFromPanel=!next&&panel.contains(focused);
  collapsedBar.inert=next;
  panel.inert=!next;
  if(cameFromMini)$('.tab.active')?.focus({preventScroll:true});
  if(cameFromPanel)$('#expandTarget')?.focus({preventScroll:true});
  if(next){
    refreshIslandHeight();
    requestAnimationFrame(()=>{resizeWave();drawWave();startWaveLoop();syncTabIndicator()});
  }else{
    island.style.setProperty('--island-height','49px');
    // Fade the last rendered silhouette with CSS while the capsule shrinks.
    // Stop RAF immediately: a moving but invisible wave wastes GPU cycles.
    if(typeof waveRaf!=='undefined'&&waveRaf){
      cancelAnimationFrame(waveRaf);
      waveRaf=0
    }
    motion?.clearLyrics();
  }
  const duration=motion?.motionAllowed(options.reducedMotion)
    ?motion.timings().morph:0;
  morphTimer=setTimeout(()=>{
    if(token!==morphSequence)return;
    island.classList.remove('is-morphing');
    if(next)refreshIslandHeight();
    else clearWave();
  },duration+60);
}
let expandRequest=0;
async function setExpanded(value){
  const next=Boolean(value);
  const request=++expandRequest;
  if(next===expanded&&island.classList.contains('collapsed')===!next){
    // Another resize may still be queued: reconcile the actual Windows size
    // even when the visible CSS capsule already looks correct.
    if(nativeShell)await resizeNativeWindow(next);
    return
  }
  // Grow the transparent Windows viewport first. The actual visible pill
  // then morphs inside it rather than flashing a full-sized panel.
  if(next&&nativeShell){
    const resized=await resizeNativeWindow(true);
    if(request!==expandRequest)return;
    if(!resized){
      toast('Не удалось раскрыть окно Windows');
      return
    }
  }
  commitExpanded(next);
  if(!next&&nativeShell){
    // Keep the full native viewport during the shrinking capsule animation.
    // Only shrink Windows' hit-test region after the visible morph ends.
    const duration=motion?.motionAllowed(options.reducedMotion)
      ?motion.timings().morph:0;
    await motion?.onceTransition(island,'width',duration);
    if(request!==expandRequest)return;
    await resizeNativeWindow(false)
  }
}
function showTab(name){
  const nextView=$('#view-'+name);
  if(!nextView)return;
  const ticket=++tabRequest;
  const change=()=>{
    // View Transitions can defer their update callback. Ignore an obsolete
    // one if a newer tab was selected while the old transition was pending.
    if(ticket!==tabRequest)return;
    tab=name;
    $$('.tab').forEach(button=>{
      const active=button.dataset.tab===name;
      button.classList.toggle('active',active);
      button.setAttribute('aria-selected',String(active));
      button.tabIndex=active?0:-1
    });
    $$('.tab-content').forEach(view=>{
      const active=view===nextView;
      view.classList.toggle('active',active);
      view.inert=!active
    });
    if(name==='shots')requestAnimationFrame(updateEditor);
    requestAnimationFrame(()=>{
      if(ticket!==tabRequest)return;
      syncTabIndicator();refreshIslandHeight()
    })
  };
  const transitioning=tab!==name&&expanded&&
    !island.classList.contains('collapsed')&&
    motion?.motionAllowed(options.reducedMotion)&&
    typeof document.startViewTransition==='function';
  if(transitioning){
    try{
      tabTransition?.skipTransition?.();
      tabTransition=document.startViewTransition(change);
      tabTransition.finished.catch(()=>{}).finally(()=>{
        if(ticket===tabRequest)tabTransition=null
      })
    }catch(error){
      console.warn('Island Lab: view transition unavailable',error);
      change()
    }
  }else change();
  if(!expanded)void setExpanded(true)
}
nav.addEventListener('keydown',event=>{
  if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;
  const buttons=$$('.tab').filter(button=>!button.disabled);
  if(!buttons.length)return;
  const current=buttons.indexOf(document.activeElement);
  if(current<0)return;
  event.preventDefault();
  const nextIndex=event.key==='Home'?0:
    event.key==='End'?buttons.length-1:
    (current+(event.key==='ArrowRight'?1:-1)+buttons.length)%buttons.length;
  const next=buttons[nextIndex];
  next.focus({preventScroll:true});
  showTab(next.dataset.tab);
  next.scrollIntoView?.({
    block:'nearest',inline:'nearest',
    behavior:motion?.motionAllowed(options.reducedMotion)?'smooth':'auto'
  })
});
$('#expandTarget').addEventListener('click',event=>{
  event.stopPropagation();
  void setExpanded(true);
});
$('#collapsedBar').addEventListener('click',event=>{
  if(event.target.closest('.micro-player')||event.target.closest('#expandTarget'))return;
  void setExpanded(true);
});
$('#collapseButton').addEventListener('click',()=>setExpanded(false));
$$('.tab').forEach(b=>b.addEventListener('click',()=>showTab(b.dataset.tab)));
document.addEventListener('keydown',event=>{
  if(event.key!=='Escape'||!expanded||event.defaultPrevented||event.isComposing)return;
  if(document.querySelector('dialog[open]'))return;
  event.preventDefault();
  void setExpanded(false)
});
function placeIsland(left,top){const w=shell.offsetWidth||900;const l=Math.max(8,Math.min(innerWidth-w-8,left));const t=Math.max(4,Math.min(innerHeight-90,top));shell.style.left=l+'px';shell.style.top=t+'px';shell.style.transform='none'}
let dragging=null;
function startDrag(e){
  if(e.button!==0)return;
  if(nativeShell){
    const drag=nativeApi.window?.getCurrentWindow?.()?.startDragging?.();
    drag?.catch(error=>console.warn('Island Lab: native drag failed',error));
    e.preventDefault();return
  }
  const rect=shell.getBoundingClientRect();
  dragging={id:e.pointerId,dx:e.clientX-rect.left,dy:e.clientY-rect.top};
  e.currentTarget.setPointerCapture(e.pointerId);e.preventDefault()
}
function moveDrag(e){if(!dragging||dragging.id!==e.pointerId)return;const left=e.clientX-dragging.dx,top=e.clientY-dragging.dy;placeIsland(left,top)}
function stopDrag(e){if(!dragging||dragging.id!==e.pointerId)return;dragging=null;const r=shell.getBoundingClientRect();options.offset={left:r.left,top:r.top};persist()}
$('#dragHandle').addEventListener('pointerdown',startDrag);
$('#dragHandle').addEventListener('pointermove',moveDrag);
$('#dragHandle').addEventListener('pointerup',stopDrag);
$('#resetPosition').addEventListener('click',()=>{
  options.offset=null;persist();
  shell.style.left='50%';shell.style.top=nativeShell?'8px':'35px';
  shell.style.transform='translateX(-50%)';
  resizeNativeWindow(expanded);
  toast('Положение сброшено')
});
window.addEventListener('resize',()=>{
  if(options.offset&&!nativeShell)placeIsland(options.offset.left,options.offset.top);
  requestAnimationFrame(()=>{refreshIslandHeight();resizeWave();syncTabIndicator()})
});
nav.addEventListener('scroll',syncTabIndicator,{passive:true});
// Content may grow when the online weather/lyrics load or notes are edited.
// A ResizeObserver keeps the shell fitted, but never retargets the height
// during the spring morph (which would otherwise cause layout thrashing).
if(typeof ResizeObserver!=='undefined'){
  let heightRaf=0;
  const observer=new ResizeObserver(()=>{
    if(!expanded||island.classList.contains('is-morphing')||heightRaf)return;
    heightRaf=requestAnimationFrame(()=>{
      heightRaf=0;
      if(expanded&&!island.classList.contains('is-morphing'))refreshIslandHeight()
    })
  });
  observer.observe(panel);
  // The downward wave is outside the clipped capsule. Keep its visible
  // width aligned with the border while CSS animates the island geometry.
  let resizeWaveRaf=0,lastCanvasResize=0;
  const borderObserver=new ResizeObserver(()=>{
    const width=island.getBoundingClientRect().width;
    $('#waveCanvas').style.setProperty('--wave-width',Math.round(width)+'px');
    if(!expanded||resizeWaveRaf)return;
    // Avoid recreating the canvas bitmap at every sub-pixel width change
    // during the 560ms capsule morph. Its CSS size remains continuously
    // animated while bitmap resizing is capped to ~30Hz.
    const updateBitmap=now=>{
      if(!expanded){resizeWaveRaf=0;return}
      if(now-lastCanvasResize<32){
        // Defer once more rather than permanently skipping the last size:
        // the final Observer notification could arrive just below 32ms.
        resizeWaveRaf=requestAnimationFrame(updateBitmap);
        return
      }
      resizeWaveRaf=0;
      lastCanvasResize=now;
      resizeWave();
      drawWave(now)
    };
    resizeWaveRaf=requestAnimationFrame(updateBitmap)
  });
  borderObserver.observe(island)
}

/* Music engine: demo synthesizer and user-loaded files feed the SAME analyser. */
const tracks=[
 {name:'Polar Echoes',artist:'Island Lab — Demo',duration:48,notes:[261.63,329.63,392,523.25,440,392,329.63,293.66],lyrics:[[0,'Слышишь, как идёт снег?'],[5,'Город засыпает'],[10,'А музыка кружится'],[15,'Между тихих улиц'],[20,'В нашем маленьком мире'],[25,'Время замедляется'],[30,'И сияет островок'],[36,'В ритме зимней ночи'],[42,'До нового рассвета']]},
 {name:'Blue Midnight',artist:'Island Lab — Demo',duration:48,notes:[220,261.63,329.63,392,369.99,329.63,261.63,196],lyrics:[[0,'За окном мерцает свет'],[6,'Небо медленно темнеет'],[12,'Слышен ровный ритм'],[18,'И волна уходит вниз'],[24,'Всё становится спокойней'],[30,'Новый день совсем близко'],[36,'Пусть эта мелодия звучит'],[42,'Пока город ещё спит']]}
];
const audio=$('#audio');
const music={demoIndex:0,mode:'demo',playing:false,ctx:null,analyser:null,source:null,bus:null,fft:null,timer:null,beat:0,nextBeat:0,start:0,offset:0,lyrics:tracks[0].lyrics.map(([at,text])=>({at,text})),lyricIndex:-1,files:[],fileIndex:0,lyricsRequest:0,systemPosition:0,systemDuration:0,systemAt:0,systemTrackKey:''};
function activeTrack(){return tracks[music.demoIndex]}
function duration(){
  if(music.mode==='system')return music.systemDuration;
  return music.mode==='file'?(Number.isFinite(audio.duration)?audio.duration:0):activeTrack().duration
}
function currentTime(){
  if(music.mode==='system'){
    const elapsed=music.playing?(performance.now()-music.systemAt)/1000:0;
    return Math.max(0,Math.min(duration()||Infinity,music.systemPosition+Math.max(0,elapsed)))
  }
  return music.mode==='file'?(audio.currentTime||0):(music.playing?(performance.now()/1000-music.start):music.offset)
}
function formatTime(sec){sec=Math.max(0,Math.floor(sec||0));return Math.floor(sec/60)+':'+String(sec%60).padStart(2,'0')}
function audioInit(){if(music.ctx)return true;const C=window.AudioContext||window.webkitAudioContext;if(!C){toast('Web Audio недоступен');return false}music.ctx=new C();music.bus=music.ctx.createGain();music.bus.gain.value=.65;music.analyser=music.ctx.createAnalyser();music.analyser.fftSize=256;music.analyser.smoothingTimeConstant=.72;music.fft=new Uint8Array(music.analyser.frequencyBinCount);music.bus.connect(music.analyser);music.analyser.connect(music.ctx.destination);return true}
function note(freq,time,length=.26,volume=.075,type='sine'){const ctx=music.ctx,o=ctx.createOscillator(),g=ctx.createGain();o.type=type;o.frequency.setValueAtTime(freq,time);g.gain.setValueAtTime(.0001,time);g.gain.exponentialRampToValueAtTime(volume,time+.012);g.gain.exponentialRampToValueAtTime(.0001,time+length);o.connect(g);g.connect(music.bus);o.start(time);o.stop(time+length+.01)}
function schedule(){if(!music.playing||music.mode!=='demo'||!music.ctx)return;while(music.nextBeat<music.ctx.currentTime+.13){const i=music.beat%8;const n=activeTrack().notes;note(n[i],music.nextBeat,.28,.09,i%2?'triangle':'sine');if(i%2===0)note(n[(i+3)%8]/2,music.nextBeat,.38,.12,'triangle');if(i%4===0)note(64,music.nextBeat,.18,.11,'sine');music.nextBeat+=.35;music.beat++}}
async function play(){if(music.mode==='system'){await controlSystemMusic('play');return}if(!audioInit())return;try{await music.ctx.resume();if(music.mode==='file'){if(!music.source){music.source=music.ctx.createMediaElementSource(audio);music.source.connect(music.bus)}await audio.play()}else{music.start=performance.now()/1000-music.offset;music.beat=Math.floor(music.offset/.35);music.nextBeat=music.ctx.currentTime;clearInterval(music.timer);music.timer=setInterval(schedule,75);schedule()}music.playing=true;updatePlayer();startWaveLoop()}catch(e){toast('Не удалось воспроизвести: '+e.message)}}
function pause(){if(music.mode==='system'){controlSystemMusic('pause');return}if(music.mode==='file')audio.pause();else music.offset=currentTime();music.playing=false;clearInterval(music.timer);updatePlayer();drawWave()}
function toggle(){if(music.mode==='system'){controlSystemMusic('toggle');return}music.playing?pause():play()}
function loadDemo(index){pause();music.mode='demo';music.demoIndex=(index+tracks.length)%tracks.length;music.offset=0;music.lyricIndex=-1;music.lyricsRequest++;music.lyrics=activeTrack().lyrics.map(([at,text])=>({at,text}));animateLabel('#trackName',activeTrack().name);animateLabel('#trackArtist',activeTrack().artist);animateCoverChange();$('#lyricsTrack').value=activeTrack().name;$('#lyricsArtist').value=activeTrack().artist;$('#lyricStatus').textContent='Встроенный демо-текст';$('#queueCount').textContent='DEMO '+(music.demoIndex+1)+'/'+tracks.length;updatePlayer()}
function parseLocalTrack(filename){
  const base=filename.replace(/\.[^.]+$/,'');
  const match=base.match(/^(.+?)\s+[-–—]\s+(.+)$/);
  return {artist:match?.[1]?.trim()||'',title:match?.[2]?.trim()||base};
}
async function useFileTrack(index,shouldPlay){
  if(!music.files.length)return;
  pause();
  music.mode='file';
  music.fileIndex=(index+music.files.length)%music.files.length;
  const item=music.files[music.fileIndex];
  audio.src=item.url;audio.load();
  music.lyrics=[];music.lyricIndex=-1;music.lyricsRequest++;
  const meta=parseLocalTrack(item.name);
  animateLabel('#trackName',meta.title);
  animateLabel('#trackArtist',meta.artist||'Локальный файл');
  animateCoverChange();
  $('#lyricsTrack').value=meta.title;
  $('#lyricsArtist').value=meta.artist;
  $('#lyricStatus').textContent='Загрузи LRC или найди текст';
  $('#queueCount').textContent=(music.fileIndex+1)+'/'+music.files.length;
  updatePlayer();
  if(shouldPlay)await play();
}
function changeTrack(delta){
  if(music.mode==='system'){controlSystemMusic(delta>0?'next':'previous');return}
  const wasPlaying=music.playing;
  if(music.mode==='file'){
    if(music.files.length>1)useFileTrack(music.fileIndex+delta,wasPlaying);
    else if(delta<0)seek(0);
    else toast('Добавь несколько файлов, чтобы переключать треки');
    return;
  }
  loadDemo(music.demoIndex+delta);
  if(wasPlaying)play()
}
function seek(time){
  time=Math.max(0,Math.min(duration()||0,time));
  if(music.mode==='system'){controlSystemMusic('seek',time);return}
  if(music.mode==='file')audio.currentTime=time;
  else{music.offset=time;if(music.playing){music.start=performance.now()/1000-time;music.beat=Math.floor(time/.35);music.nextBeat=music.ctx.currentTime}}
  updatePlayer()
}
$$('[data-player]').forEach(el=>el.addEventListener('click',e=>{e.stopPropagation();const a=el.dataset.player;if(a==='toggle')toggle();else changeTrack(a==='next'?1:-1)}));
$('#seek').addEventListener('input',e=>seek(+e.target.value/100*duration()));
$('#musicFile').addEventListener('change',async e=>{
  const files=Array.from(e.target.files||[])
    .filter(file=>file.type.startsWith('audio/')).slice(0,50);
  if(!files.length)return;
  if(music.mode==='system')disableSystemMusic();
  pause();
  for(const item of music.files)URL.revokeObjectURL(item.url);
  music.files=files.map(file=>({name:file.name,url:URL.createObjectURL(file)}));
  music.fileIndex=0;
  await useFileTrack(0,true);
  toast(music.files.length>1?'Очередь загружена: '+music.files.length+' треков':'Трек загружен');
  e.target.value='';
});
audio.addEventListener('ended',()=>{
  if(music.mode==='file'&&music.files.length>1){
    useFileTrack(music.fileIndex+1,true);
    return;
  }
  music.playing=false;updatePlayer();drawWave()
});
function parseLRC(text){const items=[];for(const line of text.replace(/\r/g,'').split('\n')){const lyric=line.replace(/\[(?:\d+:\d+(?:\.\d+)?|[^\]]+)\]/g,'').trim();for(const m of line.matchAll(/\[(\d+):(\d+(?:\.\d+)?)\]/g)){if(lyric)items.push({at:+m[1]*60+ +m[2],text:lyric})}}return items.sort((a,b)=>a.at-b.at)}
$('#lyricsFile').addEventListener('change',async e=>{const file=e.target.files?.[0];if(!file)return;const data=parseLRC(await file.text());if(!data.length){toast('Не найдены строки [mm:ss.xx]');return}music.lyricsRequest++;music.lyrics=data;music.lyricIndex=-1;$('#lyricStatus').textContent=file.name;updateLyrics();toast('LRC загружен: '+data.length+' строк')});
function updateLyrics(){
  const time=Math.max(0,currentTime()-options.lyricsOffset);
  const lines=music.lyrics;
  let index=-1;
  for(let i=0;i<lines.length;i++){
    if(lines[i].at<=time)index=i;
    else break
  }
  if(index===music.lyricIndex)return;
  const direction=index<music.lyricIndex?-1:1;
  music.lyricIndex=index;
  const text=[
    index>=1?lines[index-1].text:'—',
    index>=0?lines[index].text:'Музыка — часть атмосферы',
    lines[index+1]?.text||'—'
  ];
  const elements=[$('#lyricPast'),$('#lyricPresent'),$('#lyricFuture')];
  motion?.updateLyrics($('.lyric-drum'),elements,text,{
    direction,
    enabled:expanded&&tab==='home'&&!options.reducedMotion
  });
  if(!motion)elements.forEach((el,i)=>{el.textContent=text[i]});
}
/**
 * Select LRCLIB results strictly by metadata. Do not silently display the
 * unrelated first search result as if it matched the current track.
 */
function chooseSyncedRecord(records,title,artist,length){
  const normalized=value=>String(value||'').normalize('NFKC').toLocaleLowerCase().replace(/[^\p{L}\p{N}]+/gu,' ').trim();
  const target=normalized(title),performer=normalized(artist);
  const matches=(Array.isArray(records)?records:[]).filter(item=>{
    if(typeof item?.syncedLyrics!=='string'||!item.syncedLyrics.trim()||item.instrumental)return false;
    const named=normalized(item.trackName||item.name);
    const band=normalized(item.artistName);
    return named===target&&(band===performer||band.includes(performer)&&performer.length>=3);
  });
  matches.sort((a,b)=>{
    const aDuration=length>0&&Number.isFinite(a.duration)?Math.abs(a.duration-length):0;
    const bDuration=length>0&&Number.isFinite(b.duration)?Math.abs(b.duration-length):0;
    return aDuration-bDuration;
  });
  return matches[0]||null;
}
let lyricsSearchRunning=false;
let lyricsRetryUntil=0;
async function searchOnlineLyrics(){
  if(lyricsSearchRunning)return;
  if(Date.now()<lyricsRetryUntil){
    toast('LRCLIB ограничил запросы. Повтори через '+Math.ceil((lyricsRetryUntil-Date.now())/1000)+' с');
    return;
  }
  const title=$('#lyricsTrack').value.trim(),artist=$('#lyricsArtist').value.trim();
  if(title.length<2||artist.length<2){toast('Введи название и исполнителя');return}
  const button=$('#findLyrics');
  lyricsSearchRunning=true;button.disabled=true;$('#lyricStatus').textContent='Поиск LRCLIB…';
  const request=++music.lyricsRequest,controller=new AbortController(),abortTimer=setTimeout(()=>controller.abort(),9500);
  try{
    const query=new URLSearchParams({track_name:title,artist_name:artist});
    const response=await fetch('https://lrclib.net/api/search?'+query,{
      headers:{'Accept':'application/json','Lrclib-Client':'Island Lab/0.2 (https://github.com/Deenfoool/snowy-New-Year)'},
      signal:controller.signal
    });
    if(!response.ok){
      if(response.status===429){
        const retry=Number(response.headers.get('Retry-After'));
        const seconds=Number.isFinite(retry)&&retry>0?Math.min(600,Math.ceil(retry)):60;
        lyricsRetryUntil=Date.now()+seconds*1000;
        throw Error('Слишком много запросов. Повтори через '+seconds+' с');
      }
      throw Error('LRCLIB вернул HTTP '+response.status);
    }
    const records=await response.json();
    if(music.lyricsRequest!==request)return;
    const matched=chooseSyncedRecord(records,title,artist,duration());
    if(!matched){$('#lyricStatus').textContent='Синхронный текст не найден';toast('В LRCLIB нет точного синхронного текста для этого трека');return}
    const parsed=parseLRC(matched.syncedLyrics);
    if(!parsed.length){$('#lyricStatus').textContent='Текст без меток времени';return}
    music.lyrics=parsed;music.lyricIndex=-1;$('#lyricStatus').textContent='LRCLIB · '+parsed.length+' строк';
    updateLyrics();toast('Слова загружены из LRCLIB');
  }catch(error){
    if(music.lyricsRequest!==request)return;
    $('#lyricStatus').textContent='Не удалось получить текст';
    toast(error.name==='AbortError'?'LRCLIB не ответил вовремя':error.message);
  }finally{
    clearTimeout(abortTimer);lyricsSearchRunning=false;button.disabled=false;
  }
}
$('#findLyrics').addEventListener('click',searchOnlineLyrics);
function shiftLyrics(delta){
  const old=Number.isFinite(options.lyricsOffset)?options.lyricsOffset:0;
  options.lyricsOffset=Math.round(Math.max(-10,Math.min(10,old+delta))*10)/10;
  persist();$('#lyricsOffset').textContent=(options.lyricsOffset>0?'+':'')+options.lyricsOffset.toFixed(1)+' с';
  music.lyricIndex=-1;updateLyrics();
}
$('#lyricsEarlier').addEventListener('click',()=>shiftLyrics(-0.5));
$('#lyricsLater').addEventListener('click',()=>shiftLyrics(0.5));
/* The system player is an explicit native-only opt-in.
 * The Web Audio analyser never receives audio from other programs; while
 * controlling an external session, the wave remains flat until WASAPI exists.
 */
const systemMedia=window.IslandSystemMedia;
const audioCapture=window.IslandAudioCapture;
const systemSwitch=$('#systemPlayerToggle');
const systemStatus=$('#systemPlayerStatus');
const waveSwitch=$('#systemWaveToggle');
const captureStatus=$('#systemWaveStatus');
waveSwitch.disabled=true;

async function stopSystemWave(){
  waveSwitch.checked=false;
  captureStatus.textContent='Захват выключен';
  if(audioCapture?.active||audioCapture?.starting)await audioCapture.stop();
  drawWave();
}
function nativeSystemAudioActive(){
  return Boolean(music.mode==='system'&&audioCapture?.active
    &&audioCapture.lastAt>0
    &&performance.now()-audioCapture.lastAt<1200);
}
waveSwitch.addEventListener('change',async()=>{
  if(!waveSwitch.checked){
    await stopSystemWave();return
  }
  if(music.mode!=='system'||!nativeShell||!audioCapture?.supported){
    waveSwitch.checked=false;
    toast('Сначала подключи системный проигрыватель Windows');
    return
  }
  captureStatus.textContent='Подключение WASAPI…';
  const ok=await audioCapture.start();
  if(ok&&waveSwitch.checked&&music.mode==='system'){
    captureStatus.textContent='WASAPI: захват общего звука активен';
    startWaveLoop();
  }else{
    if(audioCapture?.active||audioCapture?.starting)await audioCapture.stop();
    waveSwitch.checked=false;
    captureStatus.textContent='WASAPI недоступен на этом устройстве';
  }
});
window.addEventListener('island:audio-capture-error',event=>{
  captureStatus.textContent='Ошибка WASAPI: '+String(event.detail.error).slice(0,96);
  waveSwitch.checked=false;
  drawWave();
});
window.addEventListener('island:audio-capture-state',event=>{
  if(!event.detail.active){
    waveSwitch.checked=false;
    if(!captureStatus.textContent.startsWith('Ошибка'))
      captureStatus.textContent='Захват выключен';
    drawWave();
  }
});
window.addEventListener('island:audio-levels',()=>{
  if(music.mode==='system'&&waveSwitch.checked&&music.playing){
    startWaveLoop();
  }
});
function disableSystemMusic(){
  void stopSystemWave();
  waveSwitch.disabled=true;
  systemMedia?.disable();
  systemSwitch.checked=false;
  systemStatus.textContent='Не подключён';
  if(music.mode==='system'){
    music.mode='demo';
    music.playing=false;
    music.systemPosition=0;music.systemDuration=0;music.systemTrackKey='';
    loadDemo(music.demoIndex);
  }
}
async function controlSystemMusic(action,seconds=null){
  if(music.mode!=='system'||!systemMedia?.enabled)return;
  const ok=await systemMedia.control(action,seconds);
  if(!ok)toast('Проигрыватель Windows не принял команду');
  systemMedia.refresh();
}
function showSystemSnapshot(snapshot,error){
  if(music.mode!=='system')return;
  if(error){
    systemStatus.textContent='Недоступно: '+String(error).slice(0,88);
    return
  }
  if(!snapshot){
    systemStatus.textContent='Нет активного совместимого плеера';
    music.playing=false;music.systemPosition=0;music.systemDuration=0;
    drawWave();
    animateLabel('#trackName','Ожидание музыки Windows');
    animateLabel('#trackArtist','Запусти совместимый проигрыватель');
    $('#queueCount').textContent='WINDOWS';
    updatePlayer();return
  }
  const key=JSON.stringify([snapshot.title,snapshot.artist,snapshot.sourceApp]);
  if(key!==music.systemTrackKey){
    music.systemTrackKey=key;
    music.lyricsRequest++;music.lyrics=[];music.lyricIndex=-1;
    animateLabel('#trackName',snapshot.title||'Без названия');
    animateLabel('#trackArtist',snapshot.artist||'Неизвестный исполнитель');
    animateCoverChange();
    $('#lyricsTrack').value=snapshot.title||'';
    $('#lyricsArtist').value=snapshot.artist||'';
    $('#lyricStatus').textContent='Найди текст через LRCLIB';
    $('#lyricPresent').textContent='Текст пока не загружен';
    $('#lyricPast').textContent='—';$('#lyricFuture').textContent='—';
  }
  music.playing=Boolean(snapshot.playing);
  music.systemDuration=Number(snapshot.durationSeconds)||0;
  music.systemPosition=Number(snapshot.positionSeconds)||0;
  music.systemAt=performance.now();
  if(music.playing)startWaveLoop();else drawWave();
  systemStatus.textContent=snapshot.sourceApp||'Windows Media Sessions';
  $('#queueCount').textContent='WINDOWS';
  updatePlayer();
}
systemSwitch.addEventListener('change',()=>{
  if(!systemSwitch.checked){disableSystemMusic();return}
  if(!nativeShell||!systemMedia?.available||!systemMedia.enable()){
    systemSwitch.checked=false;toast('Системный проигрыватель доступен только в Windows-версии');return
  }
  pause(); // stop only Island Lab's own demo or local playback
  music.mode='system';music.playing=false;music.systemTrackKey='';
  music.systemPosition=0;music.systemDuration=0;
  waveSwitch.disabled=!audioCapture?.supported;
  captureStatus.textContent=waveSwitch.disabled?'WASAPI недоступен':'Захват выключен';
  systemStatus.textContent='Подключение к Windows…';
  showSystemSnapshot(null,null);
});
window.addEventListener('island:system-media',event=>{
  if(music.mode!=='system')return;
  showSystemSnapshot(event.detail.snapshot,event.detail.error)
});
function updatePlayer(){
  const pos=currentTime(),len=duration();
  $('#timeNow').textContent=formatTime(pos);
  $('#timeEnd').textContent=formatTime(len);
  $('#seek').value=len?Math.min(100,pos/len*100):0;
  island.classList.toggle('music-playing',music.playing);
  $$('[data-player=toggle]').forEach(el=>{
    const icon=music.playing?'Ⅱ':'▶';
    if(el.textContent!==icon)el.textContent=icon;
    el.setAttribute('aria-label',music.playing?'Пауза':'Воспроизвести')
  });
  updateLyrics()
}

/* Audio-reactive bottom silhouette. Each band and the master envelope are
 * smoothed independently, so audio packets arriving at 18 Hz produce
 * continuous 60-Hz curves without random/fabricated beats.
 */
const wave=$('#waveCanvas'),wc=wave.getContext('2d');
let waveW=0,waveH=0,lastWave=0,waveRaf=0,lastProgress=0;
let cachedWaveGradient=null,cachedWaveAccent='';
const waveDynamics={
  height:0,velocity:0,
  bands:Array(16).fill(0),lastAt:0
};
function resizeWave(){
  const width=island.getBoundingClientRect().width;
  const dpr=Math.min(window.devicePixelRatio||1,2);
  waveW=Math.max(1,width);waveH=39;
  wave.style.setProperty('--wave-width',Math.round(waveW)+'px');
  const pixelsW=Math.round(waveW*dpr),pixelsH=Math.round(waveH*dpr);
  if(wave.width!==pixelsW||wave.height!==pixelsH){
    wave.width=pixelsW;wave.height=pixelsH;
    wc.setTransform(dpr,0,0,dpr,0,0);
    cachedWaveGradient=null
  }
}
function clearWave(){wc.clearRect(0,0,waveW,waveH)}
function getWaveGradient(){
  // Rebuilding a CanvasGradient and reading computed styles every audio frame
  // is wasted CPU/GPU work. Recreate only after a color or canvas-size change.
  if(cachedWaveGradient&&cachedWaveAccent===options.accent)return cachedWaveGradient;
  const gradient=wc.createLinearGradient(0,0,0,waveH);
  gradient.addColorStop(0,'#182b3e');
  gradient.addColorStop(.65,'#263f55');
  gradient.addColorStop(1,options.accent);
  cachedWaveGradient=gradient;
  cachedWaveAccent=options.accent;
  return gradient
}
function waveHasTail(){
  return waveDynamics.height>.008||Math.abs(waveDynamics.velocity)>.018||
    waveDynamics.bands.some(level=>level>.01)
}
function waveSignal(){
  const system=music.mode==='system';
  if(!options.waveEnabled||!music.playing||
    (system&&!nativeSystemAudioActive()))return{rms:0,bands:null};
  if(system){
    const frame=audioCapture.frame;
    return{rms:Math.max(0,Math.min(1,frame.rms*3)),
      bands:frame.bands}
  }
  if(!music.analyser||!music.fft)return{rms:0,bands:null};
  music.analyser.getByteFrequencyData(music.fft);
  const bands=Array.from({length:16},(_,i)=>{
    const first=Math.floor(i*3),last=Math.min(music.fft.length,first+3);
    let sum=0;
    for(let j=first;j<last;j++)sum+=music.fft[j];
    return last>first?sum/((last-first)*255):0
  });
  const rms=bands.reduce((total,level)=>total+level,0)/bands.length;
  return{rms:Math.min(1,rms*2.7),bands}
}
function drawWave(now=performance.now()){
  if(!expanded)return;
  // ResizeObserver handles morph geometry in modern browsers. On older
  // engines, fall back to an on-demand width check.
  if(waveW<=1||typeof ResizeObserver==='undefined'){
    if(Math.abs(island.getBoundingClientRect().width-waveW)>1.5)resizeWave()
  }
  const delta=waveDynamics.lastAt?Math.min(.05,Math.max(.008,(now-waveDynamics.lastAt)/1000)):1/60;
  waveDynamics.lastAt=now;
  const allowed=options.waveEnabled&&!options.reducedMotion&&
    motion?.motionAllowed(options.reducedMotion)!==false;
  const signal=allowed?waveSignal():{rms:0,bands:null};
  const target=allowed?Math.min(1,signal.rms)*(options.wavePower/100):0;
  if(!allowed){
    waveDynamics.height=0;waveDynamics.velocity=0;waveDynamics.bands.fill(0)
  }else{
    const step=motion?.spring(waveDynamics.height,waveDynamics.velocity,
      target,delta,3.65,.83)||{x:target,v:0};
    waveDynamics.height=Math.max(0,Math.min(1,step.x));
    waveDynamics.velocity=step.v;
    for(let i=0;i<16;i++){
      const wanted=(signal.bands?.[i]||0);
      const response=wanted>waveDynamics.bands[i]?21:8.5;
      waveDynamics.bands[i]=motion?.smooth(
        waveDynamics.bands[i],wanted,delta,response)||wanted
    }
    if(target<.001&&waveDynamics.height<.008&&Math.abs(waveDynamics.velocity)<.018){
      waveDynamics.height=0;waveDynamics.velocity=0;
      if(waveDynamics.bands.every(band=>band<.01))waveDynamics.bands.fill(0)
    }
  }
  clearWave();
  if(!waveHasTail())return;
  const amplitude=waveDynamics.height;
  const gradient=getWaveGradient();
  wc.beginPath();wc.moveTo(0,0);
  for(let x=0;x<=waveW;x+=3){
    const t=x/waveW;
    const fade=Math.pow(Math.sin(Math.PI*t),.72);
    const frequency=t*15;
    const index=Math.min(14,Math.floor(frequency));
    const mix=frequency-index;
    const energy=waveDynamics.bands[index]*(1-mix)
      +waveDynamics.bands[index+1]*mix;
    // Harmonic detail describes *only* the captured energy. Silence is flat.
    const ripple=.5+.26*Math.sin(t*27+now*.007)
      +.16*Math.sin(t*61-now*.009);
    const depth=fade*amplitude*(8+27*energy+8*ripple);
    wc.lineTo(x,Math.max(0,Math.min(waveH-2,depth)))
  }
  wc.lineTo(waveW,0);wc.closePath();
  wc.fillStyle=gradient;wc.fill();
  if(amplitude>.04){
    wc.lineWidth=.7;
    wc.strokeStyle='rgba(192,235,252,.52)';
    wc.stroke()
  }
  // When pause switches playback off, retain a short physical release tail.
  if(!music.playing&&waveHasTail())startWaveLoop()
}
function frame(now){
  waveRaf=0;
  if(!expanded||document.hidden||!options.waveEnabled||
     motion?.motionAllowed(options.reducedMotion)===false){
    waveDynamics.height=0;waveDynamics.velocity=0;
    waveDynamics.bands.fill(0);clearWave();return
  }
  // Follow 60/120/144 Hz browser refresh when the display can deliver it.
  // A 6ms floor caps pathological high-rate RAFs at around 166 FPS.
  if(now-lastWave>=6){
    drawWave(now);
    if(music.playing){
      if(music.mode==='demo'&&currentTime()>=duration()){
        music.offset=0;music.start=performance.now()/1000;
        music.nextBeat=music.ctx.currentTime;music.beat=0
      }
      if(now-lastProgress>=90){
        updatePlayer();lastProgress=now
      }
    }
    lastWave=now
  }
  startWaveLoop()
}
function startWaveLoop(){
  if(waveRaf||!expanded||document.hidden||
     motion?.motionAllowed(options.reducedMotion)===false||
     !options.waveEnabled)return;
  const hasSource=music.playing&&(music.mode!=='system'||nativeSystemAudioActive());
  if(!hasSource&&!waveHasTail())return;
  waveRaf=requestAnimationFrame(frame)
}

/* Live weather is an explicit user action; browser-only Open-Meteo fetch. */
function updateWeather({temp,wind,description,city,live}){const t=(temp>0?'+':temp<0?'−':'')+Math.round(Math.abs(temp))+'°';const symbol=temp<=0?'❄':description.includes('Дожд')?'☂':'☁';animateLabel('#weatherTemp',t);animateLabel('#miniTemperature',t);animateLabel('#weatherIcon',symbol);animateLabel('#miniWeatherIcon',symbol);$('#weatherText').textContent=description;$('#weatherCity').textContent=city;$('#weatherWind').textContent='Ветер '+Math.round(wind/3.6*10)/10+' м/с';$('#weatherSource').textContent=live?'LIVE':'ДЕМО';$('.weather-demo-dot').textContent=live?'LIVE':'DEMO';$('#panelState').textContent=live?'Погода обновлена':'Демо-режим'}
function weatherCode(code){if([0,1].includes(code))return 'Ясно';if([2,3].includes(code))return 'Облачно';if([45,48].includes(code))return 'Туман';if([51,53,55,61,63,65,80,81,82].includes(code))return 'Дождь';if([71,73,75,77,85,86].includes(code))return 'Снег';if([95,96,99].includes(code))return 'Гроза';return 'Переменная облачность'}
async function fetchWeather(){const city=$('#weatherInput').value.trim();if(!city){toast('Введи город в настройках');showTab('settings');return}$('#weatherMessage').textContent='Загрузка...';try{const geo=await fetch('https://geocoding-api.open-meteo.com/v1/search?name='+encodeURIComponent(city)+'&count=1&language=ru&format=json').then(r=>{if(!r.ok)throw Error('Ошибка геокодирования');return r.json()});const loc=geo.results?.[0];if(!loc)throw Error('Город не найден');const query=new URLSearchParams({latitude:loc.latitude,longitude:loc.longitude,current:'temperature_2m,weather_code,wind_speed_10m',timezone:'auto'});const data=await fetch('https://api.open-meteo.com/v1/forecast?'+query).then(r=>{if(!r.ok)throw Error('Ошибка сервиса погоды');return r.json()});const current=data.current;if(!current)throw Error('Данные не получены');updateWeather({temp:current.temperature_2m,wind:current.wind_speed_10m,description:weatherCode(current.weather_code),city:loc.name,live:true});options.weatherCity=city;persist();$('#weatherMessage').textContent='Погода получена от Open-Meteo. Время: '+current.time;toast('Погода обновлена')}catch(err){$('#weatherMessage').textContent='Не удалось загрузить: '+err.message;toast('Погода недоступна — остаются прежние данные')}}
$('#weatherSearch').addEventListener('click',fetchWeather);$('#refreshWeather').addEventListener('click',()=>{if(options.weatherCity)fetchWeather();else showTab('settings')});

/* Screenshot editor — two stacked canvases; annotations do not damage original image. */
const base=$('#imageCanvas'),draw=$('#drawCanvas'),bc=base.getContext('2d'),dc=draw.getContext('2d');
const MAX_EDITOR_HISTORY=64*1024*1024;
const edit={
  tool:'pen',color:'#8dd9f6',size:5,undo:[],redo:[],
  drawing:false,start:null,last:null,snapshot:null,crop:null,
  cropBefore:null
};
function emptyCanvas(){
  bc.fillStyle='#142a3d';bc.fillRect(0,0,base.width,base.height);
  bc.fillStyle='#d2e9f5';bc.font='700 36px Segoe UI';bc.fillText('ISLAND LAB',60,115);
  bc.font='22px Segoe UI';bc.fillStyle='#85afca';
  bc.fillText('Загрузи изображение или сделай скриншот',60,165);
  for(let i=0;i<13;i++){bc.strokeStyle='rgba(141,217,246,'+(i%3===0?.15:.07)+')';bc.beginPath();bc.moveTo(0,260+i*32);bc.lineTo(base.width,260+i*32);bc.stroke()}
}
function setCanvasSize(w,h){
  // Keep original aspect ratio even when the image exceeds the safe
  // browser-canvas size. Avoid warping 8K screenshots to a square.
  w=Number.isFinite(w)?w:1080;h=Number.isFinite(h)?h:608;
  const scale=Math.min(1,4096/Math.max(w,h,1));
  w=Math.max(1,Math.round(w*scale));h=Math.max(1,Math.round(h*scale));
  base.width=draw.width=w;base.height=draw.height=h;
  $('.editor-frame').style.aspectRatio=w+'/'+h;
  edit.undo=[];edit.redo=[];edit.crop=null;updateCropOutline();
}
function imageIn(img){
  setCanvasSize(img.width||img.naturalWidth,img.height||img.naturalHeight);
  bc.clearRect(0,0,base.width,base.height);bc.drawImage(img,0,0,base.width,base.height);
  dc.clearRect(0,0,draw.width,draw.height);updateEditor();showTab('shots')
}
function loadImage(file){
  if(!file||!file.type.startsWith('image/'))return;
  const url=URL.createObjectURL(file),img=new Image();
  img.onload=()=>{imageIn(img);URL.revokeObjectURL(url);toast('Изображение открыто')};
  img.onerror=()=>{URL.revokeObjectURL(url);toast('Не удалось прочитать изображение')};
  img.src=url
}
$('#imageFile').addEventListener('change',e=>loadImage(e.target.files?.[0]));
document.addEventListener('paste',e=>{
  const item=[...(e.clipboardData?.items||[])].find(x=>x.type.startsWith('image/'));
  if(item){e.preventDefault();loadImage(item.getAsFile())}
});
document.addEventListener('keydown',e=>{
  if(e.key==='PrintScreen'){
    showTab('shots');
    toast('Сделай снимок средствами Windows и вставь его Ctrl+V.')
  }
  if(tab==='shots'&&expanded&&(e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='z'){
    e.preventDefault();if(e.shiftKey)redo();else undo()
  }
});
$('#pasteHelp').addEventListener('click',async()=>{
  try{
    const items=await navigator.clipboard.read();
    for(const item of items){
      const type=item.types.find(x=>x.startsWith('image/'));
      if(type){loadImage(new File([await item.getType(type)],'clipboard.png',{type}));return}
    }
    toast('В буфере нет изображения — попробуй Ctrl+V')
  }catch{toast('Нажми Ctrl+V, чтобы вставить изображение')}
});
async function capture(){
  if(!navigator.mediaDevices?.getDisplayMedia){
    toast('Захват доступен только на HTTPS или localhost. Можно загрузить файл.');return
  }
  let stream;
  try{
    stream=await navigator.mediaDevices.getDisplayMedia({video:true,audio:false});
    const video=document.createElement('video');video.srcObject=stream;video.muted=true;
    await video.play();
    if(!video.videoWidth)await new Promise(resolve=>video.addEventListener('loadedmetadata',resolve,{once:true}));
    setCanvasSize(video.videoWidth,video.videoHeight);
    bc.drawImage(video,0,0,base.width,base.height);
    dc.clearRect(0,0,draw.width,draw.height);
    showTab('shots');toast('Скриншот готов к редактированию')
  }catch(e){
    toast(e.name==='NotAllowedError'?'Пользователь отменил захват':'Не удалось захватить экран: '+e.message)
  }finally{stream?.getTracks().forEach(track=>track.stop())}
}
$('#captureScreen').addEventListener('click',capture);
function canvasPos(e){
  const r=draw.getBoundingClientRect();
  return {x:(e.clientX-r.left)*draw.width/r.width,y:(e.clientY-r.top)*draw.height/r.height}
}
function cropRectFromPoints(a,b,width,height){
  // Clamp both corners before normalizing. Returning null prevents
  // zero-area selections from producing empty PNG exports.
  const x1=Math.max(0,Math.min(width,Math.round(a.x)));
  const x2=Math.max(0,Math.min(width,Math.round(b.x)));
  const y1=Math.max(0,Math.min(height,Math.round(a.y)));
  const y2=Math.max(0,Math.min(height,Math.round(b.y)));
  const rect={x:Math.min(x1,x2),y:Math.min(y1,y2),w:Math.abs(x2-x1),h:Math.abs(y2-y1)};
  return rect.w>=5&&rect.h>=5?rect:null
}
function updateCropOutline(){
  const el=$('#cropOutline'),crop=edit.crop;
  el.hidden=!crop;
  if(!crop)return;
  el.style.left=(crop.x/base.width*100)+'%';
  el.style.top=(crop.y/base.height*100)+'%';
  el.style.width=(crop.w/base.width*100)+'%';
  el.style.height=(crop.h/base.height*100)+'%'
}
function editorState(pixels=true){
  return {pixels:pixels?dc.getImageData(0,0,draw.width,draw.height):null,crop:edit.crop?{...edit.crop}:null}
}
function trimHistory(list){
  // Prevent full-screen snapshots from consuming gigabytes of RAM.
  let bytes=list.reduce((acc,entry)=>acc+(entry.pixels?.data?.byteLength||128),0);
  while(list.length>1&&bytes>MAX_EDITOR_HISTORY){
    const entry=list.shift();bytes-=entry.pixels?.data?.byteLength||128
  }
}
function pushUndo(pixels=true){
  const state=editorState(pixels);
  edit.undo.push(state);trimHistory(edit.undo);edit.redo.length=0;
  return state.pixels
}
function styleStroke(){
  dc.lineWidth=edit.tool==='marker'?edit.size*4:edit.size;
  dc.lineCap='round';dc.lineJoin='round';
  dc.strokeStyle=edit.color;dc.fillStyle=edit.color;
  dc.globalAlpha=edit.tool==='marker'?.32:1;
  dc.globalCompositeOperation=edit.tool==='eraser'?'destination-out':'source-over'
}
function line(a,b){dc.beginPath();dc.moveTo(a.x,a.y);dc.lineTo(b.x,b.y);dc.stroke()}
function arrow(a,b){
  line(a,b);const angle=Math.atan2(b.y-a.y,b.x-a.x),len=Math.max(13,edit.size*3);
  line(b,{x:b.x-len*Math.cos(angle-.48),y:b.y-len*Math.sin(angle-.48)});
  line(b,{x:b.x-len*Math.cos(angle+.48),y:b.y-len*Math.sin(angle+.48)})
}
function shape(a,b){
  dc.save();styleStroke();
  if(edit.tool==='arrow')arrow(a,b);
  if(edit.tool==='rect')dc.strokeRect(a.x,a.y,b.x-a.x,b.y-a.y);
  if(edit.tool==='ellipse'){
    dc.beginPath();
    dc.ellipse((a.x+b.x)/2,(a.y+b.y)/2,Math.max(1,Math.abs(a.x-b.x)/2),Math.max(1,Math.abs(a.y-b.y)/2),0,0,Math.PI*2);
    dc.stroke()
  }
  dc.restore()
}
draw.addEventListener('pointerdown',e=>{
  if(e.button!==0)return;
  const p=canvasPos(e);
  if(edit.tool==='text'){
    const txt=prompt('Текст на изображении');if(!txt)return;
    pushUndo();dc.save();styleStroke();dc.font='700 '+Math.max(22,edit.size*5)+'px Segoe UI';
    dc.fillText(txt,p.x,p.y,Math.max(10,draw.width-p.x));dc.restore();updateEditor();return
  }
  edit.snapshot=pushUndo(edit.tool!=='crop');
  edit.cropBefore=edit.crop?{...edit.crop}:null;
  edit.drawing=true;edit.start=p;edit.last=p;
  draw.setPointerCapture(e.pointerId);
  if(['pen','marker','eraser'].includes(edit.tool)){
    dc.save();styleStroke();line(p,{x:p.x+.1,y:p.y+.1});dc.restore()
  }
});
draw.addEventListener('pointermove',e=>{
  if(!edit.drawing)return;
  const p=canvasPos(e);
  if(edit.tool==='crop'){
    edit.crop=cropRectFromPoints(edit.start,p,draw.width,draw.height);updateCropOutline()
  }else if(['pen','marker','eraser'].includes(edit.tool)){
    dc.save();styleStroke();line(edit.last,p);dc.restore()
  }else{
    dc.putImageData(edit.snapshot,0,0);shape(edit.start,p)
  }
  edit.last=p
});
function finishDrawing(e){
  if(!edit.drawing)return;
  if(edit.tool==='crop'){
    edit.crop=cropRectFromPoints(edit.start,canvasPos(e),draw.width,draw.height)||edit.cropBefore;
    updateCropOutline()
  }
  edit.drawing=false;edit.snapshot=null;edit.cropBefore=null;updateEditor()
}
draw.addEventListener('pointerup',finishDrawing);
draw.addEventListener('pointercancel',finishDrawing);
$$('[data-tool]').forEach(button=>button.addEventListener('click',()=>{
  edit.tool=button.dataset.tool;
  $$('[data-tool]').forEach(el=>el.classList.toggle('active',el===button));
  updateEditor()
}));
$('#penColor').addEventListener('input',e=>edit.color=e.target.value);
$('#penSize').addEventListener('input',e=>edit.size=+e.target.value);
function applyEditorState(state){
  if(state.pixels)dc.putImageData(state.pixels,0,0);
  edit.crop=state.crop;updateCropOutline();updateEditor()
}
function undo(){
  if(!edit.undo.length)return;
  const prior=edit.undo.pop();
  edit.redo.push(editorState(!!prior.pixels));trimHistory(edit.redo);
  applyEditorState(prior)
}
function redo(){
  if(!edit.redo.length)return;
  const next=edit.redo.pop();
  edit.undo.push(editorState(!!next.pixels));trimHistory(edit.undo);
  applyEditorState(next)
}
$('#undo').addEventListener('click',undo);
$('#redo').addEventListener('click',redo);
$('#clearEdits').addEventListener('click',()=>{
  pushUndo();dc.clearRect(0,0,draw.width,draw.height);updateEditor()
});
$('#removeCrop').addEventListener('click',()=>{
  if(!edit.crop)return;
  pushUndo(false);edit.crop=null;updateCropOutline();updateEditor()
});
function updateEditor(){
  const toolNames={pen:'карандаш',marker:'маркер',arrow:'стрелка',rect:'прямоугольник',ellipse:'овал',crop:'обрезка',text:'текст',eraser:'ластик'};
  const region=edit.crop?' · Обрезка: '+edit.crop.w+'×'+edit.crop.h:'';
  $('#editorStatus').textContent='Инструмент: '+toolNames[edit.tool]+' · '+draw.width+'×'+draw.height+region+' · Отмен: '+edit.undo.length;
  $('#removeCrop').hidden=!edit.crop
}
function downloadPng(blob){
  const url=URL.createObjectURL(blob),link=document.createElement('a');
  link.href=url;link.download='Island-Lab-'+Date.now()+'.png';link.click();
  setTimeout(()=>URL.revokeObjectURL(url),2000)
}
async function savePngBlob(blob){
  if(nativeShell){
    try{
      const pngData=await new Promise((resolve,reject)=>{
        const reader=new FileReader();
        reader.onload=()=>resolve(reader.result);
        reader.onerror=()=>reject(reader.error||Error('Не удалось прочитать PNG'));
        reader.readAsDataURL(blob)
      });
      const savedPath=await nativeApi.core.invoke('save_screenshot_native',{pngData});
      $('#editorStatus').textContent='Снимок сохранён в '+savedPath;
      toast('PNG сохранён в Изображения / Island Lab');
      return
    }catch(error){
      console.warn('Island Lab: native screenshot save failed',error);
      downloadPng(blob);
      toast('Не удалось записать в папку. PNG скачан браузером.');
      return
    }
  }
  downloadPng(blob);
  toast('PNG сохранён через браузер')
}
let savingScreenshot=false;
$('#saveShot').addEventListener('click',()=>{
  if(savingScreenshot)return;
  savingScreenshot=true;
  const crop=edit.crop||{x:0,y:0,w:base.width,h:base.height};
  const out=document.createElement('canvas');out.width=crop.w;out.height=crop.h;
  const ctx=out.getContext('2d');
  for(const layer of [base,draw])ctx.drawImage(layer,crop.x,crop.y,crop.w,crop.h,0,0,crop.w,crop.h);
  out.toBlob(async blob=>{
    try{
      if(!blob){toast('Не удалось сохранить PNG');return}
      await savePngBlob(blob);
    }finally{savingScreenshot=false}
  },'image/png')
});
$('#openShotsFolder').addEventListener('click',async()=>{
  if(!nativeShell)return;
  const button=$('#openShotsFolder');button.disabled=true;
  try{
    await nativeApi.core.invoke('open_screenshot_folder_native');
    toast('Папка снимков открыта в Проводнике')
  }catch(error){
    console.warn('Island Lab: cannot open screenshot folder',error);
    toast('Папка недоступна: '+String(error).slice(0,90))
  }finally{button.disabled=false}
});

/* Notes — localStorage, no external account. */
let notes=[];let selectedNote=null;let notesSaveTimer=null;
try{notes=JSON.parse(localStorage.getItem(NOTES)||'[]');if(!Array.isArray(notes))notes=[]}catch{notes=[]}
if(!notes.length)notes=[{id:Date.now(),title:'Первая заметка',body:'Island Lab — новая идея для рабочего стола.\n\nСюда можно записывать мысли и планы.'}];
selectedNote=notes[0].id;
function storeNotes(){try{localStorage.setItem(NOTES,JSON.stringify(notes));$('#noteSaved').textContent='Сохранено локально'}catch{$('#noteSaved').textContent='Ошибка сохранения'}}
function renderNotes(){const list=$('#notesList');list.replaceChildren();for(const note of notes){const b=document.createElement('button');b.type='button';b.className='note-item'+(note.id===selectedNote?' active':'');b.textContent=note.title||'Без названия';b.addEventListener('click',()=>{selectedNote=note.id;renderNotes()});list.append(b)}const n=notes.find(x=>x.id===selectedNote);$('#noteTitle').value=n?.title||'';$('#noteBody').value=n?.body||'';$('#deleteNote').disabled=notes.length===1}
function saveNote(){const n=notes.find(x=>x.id===selectedNote);if(!n)return;n.title=$('#noteTitle').value;n.body=$('#noteBody').value;$('#noteSaved').textContent='Сохранение…';clearTimeout(notesSaveTimer);notesSaveTimer=setTimeout(()=>{storeNotes();$$('.note-item').forEach((button,index)=>{const note=notes[index];if(note)button.textContent=note.title||'Без названия'})},370)}
$('#noteTitle').addEventListener('input',saveNote);$('#noteBody').addEventListener('input',saveNote);
$('#addNote').addEventListener('click',()=>{const n={id:Date.now(),title:'Новая заметка',body:''};notes.unshift(n);selectedNote=n.id;storeNotes();renderNotes();$('#noteTitle').focus();$('#noteTitle').select()});
$('#deleteNote').addEventListener('click',()=>{if(notes.length<2)return;notes=notes.filter(x=>x.id!==selectedNote);selectedNote=notes[0].id;storeNotes();renderNotes();toast('Заметка удалена')});

/* Snow effect behind the widget is isolated from the Snowy New Year process. */
const snow=document.createElement('canvas');snow.id='previewSnow';Object.assign(snow.style,{position:'absolute',inset:'0',width:'100%',height:'100%',pointerEvents:'none',zIndex:'1'});$('#desktop').append(snow);const sc=snow.getContext('2d');const flakes=Array.from({length:90},()=>({x:Math.random(),y:Math.random(),r:.55+Math.random()*2.1,v:.12+Math.random()*.52}));let snowFrame=0,snowRaf=0;
function snowEnabled(){
  return options.snow&&options.backgroundSnow&&
    motion?.motionAllowed(options.reducedMotion)!==false&&
    !nativeShell&&!document.hidden&&options.snowIntensity>0
}
function renderSnow(time){
  snowRaf=0;
  if(!snowEnabled())return;
  const w=innerWidth,h=innerHeight,dpr=Math.min(devicePixelRatio||1,2);
  const frameSeconds=snowFrame?Math.max(0,Math.min(.05,(time-snowFrame)/1000)):1/60;
  snowFrame=time;
  if(snow.width!==Math.round(w*dpr)||snow.height!==Math.round(h*dpr)){
    snow.width=Math.round(w*dpr);snow.height=Math.round(h*dpr);
    sc.setTransform(dpr,0,0,dpr,0,0)
  }
  sc.clearRect(0,0,w,h);
  sc.fillStyle='rgba(218,241,255,.74)';
  const count=Math.floor(flakes.length*options.snowIntensity/100);
  for(let i=0;i<count;i++){
    const flake=flakes[i];
    // Time-based motion yields the same speed on 60/120/144 Hz screens.
    flake.y+=flake.v*24*frameSeconds/h;
    flake.x+=Math.sin(time*.0002+flake.y*12)*.13*24*frameSeconds/w;
    if(flake.y>1){flake.y=0;flake.x=Math.random()}
    if(flake.x>1)flake.x=0;
    if(flake.x<0)flake.x=1;
    sc.beginPath();sc.arc(flake.x*w,flake.y*h,flake.r,0,Math.PI*2);sc.fill()
  }
  snowRaf=requestAnimationFrame(renderSnow)
}
function updateSnowLoop(){
  if(!snowEnabled()){
    if(snowRaf)cancelAnimationFrame(snowRaf);snowRaf=0;
    if(snow.width)sc.clearRect(0,0,snow.width,snow.height);return
  }
  if(!snowRaf)snowRaf=requestAnimationFrame(renderSnow)
}
document.addEventListener('visibilitychange',()=>{
  updateSnowLoop();if(!document.hidden)startWaveLoop()
});
// Native Windows/Edge can change accessibility preferences without restarting
// Island. Suspend decorative loops rather than just hiding their CSS layers.
const reducedPreference=window.matchMedia?.('(prefers-reduced-motion: reduce)');
reducedPreference?.addEventListener?.('change',()=>{
  updateSnowLoop();
  if(reducedPreference.matches){
    if(waveRaf){cancelAnimationFrame(waveRaf);waveRaf=0}
    waveDynamics.height=0;waveDynamics.velocity=0;waveDynamics.bands.fill(0);
    clearWave()
  }else{
    drawWave();startWaveLoop()
  }
});

for(const [id,key,transform] of [['accentColor','accent',v=>v],['motionStyle','motionStyle',v=>v],['wavePower','wavePower',v=>+v],['waveToggle','waveEnabled',v=>v],['reducedMotion','reducedMotion',v=>v],['backgroundSnow','backgroundSnow',v=>v],['snowToggle','snow',v=>v],['capsToggle','caps',v=>v],['snowmenToggle','snowmen',v=>v],['snowIntensity','snowIntensity',v=>+v]]){$('#'+id).addEventListener(id==='accentColor'||id==='wavePower'||id==='snowIntensity'?'input':'change',e=>setOption(key,e.target.type==='checkbox'?e.target.checked:transform(e.target.value)))}
$('#weatherInput').addEventListener('change',e=>{options.weatherCity=e.target.value.trim();persist()});

/* Reproducible smoke-test report for the first Windows launch.
   Never copy note text, screenshot pixels, music file names or city names. */
/** The FPS benchmark is opt-in and runs for at most eight seconds.
 * Frame timings never leave the device; only aggregated metrics are shown.
 */
function summarizeMotionFrames(samples){
  const usable=samples.filter(ms=>Number.isFinite(ms)&&ms>0);
  if(!usable.length)return null;
  const ordered=usable.slice().sort((a,b)=>a-b);
  const total=usable.reduce((sum,value)=>sum+value,0);
  const mean=total/usable.length;
  const p95=ordered[Math.min(ordered.length-1,Math.ceil(.95*ordered.length)-1)];
  // Use a fast-but-not-minimum reference frame. At 120/144 Hz a lost
  // frame is visible well before the old fixed 33.4ms cutoff.
  const baseline=ordered[Math.floor((ordered.length-1)*.1)];
  const slowThreshold=baseline*1.75;
  const drops=usable.filter(ms=>ms>slowThreshold).length;
  return {
    fps:Math.round(1000/mean),
    meanMs:Math.round(mean*10)/10,
    p95Ms:Math.round(p95*10)/10,
    baselineMs:Math.round(baseline*10)/10,
    slowFrames:drops,
    frames:usable.length
  }
}
let lastMotionBenchmark=null,benchmarkRunning=false,benchmarkRaf=0;
const benchmarkButton=$('#motionBenchmarkBtn');
const benchmarkOutput=$('#motionBenchmarkResult');
function stopMotionBenchmark(message){
  benchmarkRunning=false;
  if(benchmarkRaf)cancelAnimationFrame(benchmarkRaf);
  benchmarkRaf=0;benchmarkButton.disabled=false;
  if(message)benchmarkOutput.textContent=message
}
benchmarkButton.addEventListener('click',()=>{
  if(benchmarkRunning)return;
  if(document.hidden){toast('Открой окно Island для проверки');return}
  benchmarkRunning=true;benchmarkButton.disabled=true;
  benchmarkOutput.textContent='Идёт замер… Переключай вкладки и запусти музыку.';
  const samples=[];
  let start=0,previous=0;
  function sample(now){
    if(!benchmarkRunning||document.hidden){
      stopMotionBenchmark('Замер остановлен: окно скрыто.');return
    }
    if(!start)start=now;
    if(previous){
      const delta=now-previous;
      if(delta>0)samples.push(delta)
    }
    previous=now;
    if(now-start<8000){
      benchmarkRaf=requestAnimationFrame(sample);return
    }
    const report=summarizeMotionFrames(samples);
    lastMotionBenchmark=report;
    stopMotionBenchmark(report?
      'FPS: '+report.fps+' · средний кадр: '+report.meanMs+
      ' мс · P95: '+report.p95Ms+' мс · базовый кадр: '+report.baselineMs+
      ' мс · рывков: '+report.slowFrames:
      'Не удалось измерить кадры.')
  }
  benchmarkRaf=requestAnimationFrame(sample)
});
document.addEventListener('visibilitychange',()=>{
  if(document.hidden&&benchmarkRunning)
    stopMotionBenchmark('Замер остановлен: окно скрыто.')
});

function diagnosticsText(){
  const features=[
    ['Tauri IPC',nativeShell?nativeBridgeStatus:'Не требуется'],
    ['Аудио Web Audio',Boolean(window.AudioContext||window.webkitAudioContext)?'Поддерживается':'Недоступно'],
    ['WASAPI Loopback',!nativeShell?'Только Windows':
      !audioCapture?.supported?'Недоступен':
      audioCapture.active?'Активен (системный микс)':'Выключен'],
    ['Спектр',audioCapture?.active?'16 полос, без сохранения звука':'Не используется'],
    ['Режим анимаций',window.IslandMotion?.preset||'fluid'],
    ['FPS (последний замер)',lastMotionBenchmark?String(lastMotionBenchmark.fps):'Не измерялся'],
    ['P95 кадра',lastMotionBenchmark?lastMotionBenchmark.p95Ms+' мс':'Не измерялся'],
    ['Редактор Canvas',Boolean(base.getContext('2d')&&draw.getContext('2d'))?'Поддерживается':'Недоступно'],
    ['Захват экрана',navigator.mediaDevices?.getDisplayMedia?'Поддерживается браузером':'Недоступно'],
    ['Буфер обмена',navigator.clipboard?.read?'Доступен при разрешении':'Ограничен'],
    ['Окно',expanded?'Развёрнуто':'Капсула'],
    ['Размещение',window.IslandDesktopMode?.mode||'normal'],
    ['Волна',options.waveEnabled?'Включена':'Отключена'],
    ['Музыка',music.playing?'Воспроизводится':'Остановлена'],
    ['Ошибки JavaScript',String(runtimeErrorCount)]
  ];
  let storage='Недоступно';
  try{const key='island-lab-storage-check';localStorage.setItem(key,'1');storage=localStorage.getItem(key)==='1'?'Доступно':'Недоступно';localStorage.removeItem(key)}catch{}
  features.push(['Локальное сохранение',storage]);
  return [
    'Island Lab — технический отчёт v0.2',
    'Режим: '+(nativeShell?'Tauri / WebView2':'Веб-страница'),
    'Размер области: '+innerWidth+'×'+innerHeight+' CSS px',
    'Масштаб экрана: '+(window.devicePixelRatio||1),
    'Контекст: '+(window.isSecureContext?'Защищённый':'Незащищённый'),
    ...features.map(([name,value])=>name+': '+value),
    'Примечание: это проверка доступности API, не гарантия работоспособности Windows-хуков.',
  ].join('\n');
}
$('#runDiagnostics').addEventListener('click',()=>{
  $('#diagnosticsOutput').textContent=diagnosticsText();
  toast('Отчёт сформирован')
});
$('#copyDiagnostics').addEventListener('click',async()=>{
  const report=diagnosticsText();
  $('#diagnosticsOutput').textContent=report;
  try{await navigator.clipboard.writeText(report);toast('Отчёт скопирован')}
  catch{toast('Не удалось скопировать автоматически — выдели отчёт и нажми Ctrl+C')}
});
$('#quitNative').hidden=!nativeShell;
$('#quitNative').addEventListener('click',()=>{
  if(!nativeShell)return;
  if(!confirm('Закрыть Island Lab?'))return;
  nativeApi.core.invoke('quit_island_lab').catch(error=>{
    console.warn('Island Lab: quit failed',error);
    toast('Не удалось закрыть приложение')
  })
});
function clock(){const now=new Date();$('#desktopClock').textContent=now.toLocaleTimeString('ru-RU',{hour:'2-digit',minute:'2-digit'});updatePlayer()}
setInterval(clock,1000);clock();emptyCanvas();updateEditor();renderNotes();applyOptions();syncTabIndicator();$('#lyricsTrack').value=activeTrack().name;$('#lyricsArtist').value=activeTrack().artist;$('#queueCount').textContent='DEMO 1/'+tracks.length;updatePlayer();setExpanded(false);updateSnowLoop();
