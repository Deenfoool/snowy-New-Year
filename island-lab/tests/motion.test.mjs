import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {Script,runInNewContext} from 'node:vm';

const root=join(dirname(fileURLToPath(import.meta.url)),'..');
const code=readFileSync(join(root,'motion.js'),'utf8');
const html=readFileSync(join(root,'index.html'),'utf8');
const css=readFileSync(join(root,'styles.css'),'utf8');
const app=readFileSync(join(root,'app.js'),'utf8');

function engine(reduced=false){
  const document={documentElement:{dataset:{}},createElement:()=>({})};
  const window={matchMedia:()=>({matches:reduced})};
  runInNewContext(code,{window,document,setTimeout,clearTimeout});
  return {motion:window.IslandMotion,document};
}

test('motion script parses and is loaded before the UI',()=>{
  assert.doesNotThrow(()=>new Script(code));
  assert.ok(html.indexOf('src="./motion.js"')<html.indexOf('src="./app.js"'));
});

test('spring simulation is stable and frame-rate independent',()=>{
  const {motion}=engine();
  const results=[30,60,120].map(fps=>{
    let state={x:0,v:0};
    for(let step=0;step<fps;step++){
      state=motion.spring(state.x,state.v,1,1/fps,3.5,.82)
    }
    return state
  });
  for(const state of results){
    assert.ok(Math.abs(state.x-1)<.002,'position should settle near target');
    assert.ok(Math.abs(state.v)<.005,'velocity should settle');
  }
  assert.ok(Math.abs(results[0].x-results[2].x)<.0005);
});

test('rapid target reversal never creates NaN or persistent drift',()=>{
  const {motion}=engine();
  let state={x:0,v:0};
  for(let i=0;i<10;i++){
    for(let j=0;j<6;j++){
      state=motion.spring(state.x,state.v,i%2?0:1,1/60,3.65,.83)
    }
  }
  for(let i=0;i<65;i++)state=motion.spring(state.x,state.v,0,1/60,3.65,.83);
  assert.ok(Number.isFinite(state.x)&&Number.isFinite(state.v));
  assert.ok(Math.abs(state.x)<.01);
});

test('motion settings validate profile, fallback and OS reduced-motion',()=>{
  const {motion,document}=engine(false);
  assert.equal(motion.setPreset('quick'),'quick');
  assert.equal(motion.timings().morph,390);
  assert.equal(document.documentElement.dataset.motion,'quick');
  assert.equal(motion.setPreset('calm'),'calm');
  assert.equal(motion.timings().morph,700);
  assert.equal(motion.setPreset('unsupported'),'fluid');
  assert.equal(motion.motionAllowed(),true);
  assert.equal(motion.motionAllowed(true),false);
  assert.equal(engine(true).motion.motionAllowed(),false);
});

test('damping approaches targets without overshooting for simple controls',()=>{
  const {motion}=engine();
  let value=0;
  for(let i=0;i<100;i++){
    value=motion.smooth(value,1,1/60,12);
    assert.ok(value>=0&&value<=1)
  }
  assert.ok(value>.99);
});

test('lyric transitions update text even when animation is disabled',()=>{
  const {motion}=engine();
  const lines=['old 1','old 2','old 3'].map(text=>({textContent:text}));
  motion.updateLyrics({},lines,['new 1','new 2','new 3'],{enabled:false});
  assert.deepEqual(lines.map(el=>el.textContent),['new 1','new 2','new 3']);
});

test('capsule morph does not use display:none and respects reduced motion',()=>{
  assert.match(css,/--motion-morph:560ms/);
  assert.match(css,/\.island\.collapsed \.expanded-panel\{/);
  assert.match(css,/\.island \.expanded-panel\{[\s\S]*?display:flex!important/);
  assert.match(css,/html\.motion-reduced/);
  assert.match(html,/id="motionStyle"/);
  assert.match(app,/refreshIslandHeight\(/);
  assert.match(app,/onceTransition\(island,'width'/);
  assert.match(app,/document\.startViewTransition\(change\)/);
  assert.match(app,/syncTabIndicator\(/);
});

test('wave is driven by smoothed real bands and runs only while needed',()=>{
  assert.match(app,/const waveDynamics=/);
  assert.match(app,/motion\?\.spring\(waveDynamics\.height/);
  assert.match(app,/motion\?\.smooth\(/);
  assert.match(app,/function waveHasTail\(/);
  assert.match(app,/function startWaveLoop\(/);
  assert.match(app,/if\(!hasSource&&!waveHasTail\(\)\)return/);
  assert.match(app,/now-lastProgress>=90/);
});

test('frame pacing summary reports mean FPS, p95 and jank',()=>{
  const start=app.indexOf('function summarizeMotionFrames(');
  const end=app.indexOf('let lastMotionBenchmark=',start);
  assert.ok(start>=0&&end>start);
  const summarize=runInNewContext(app.slice(start,end)+'\nsummarizeMotionFrames',{});
  const normal=summarize(Array(60).fill(1000/60));
  assert.equal(normal.fps,60);
  assert.equal(normal.slowFrames,0);
  assert.ok(normal.p95Ms>16&&normal.p95Ms<17);
  const janky=summarize([...Array(19).fill(16.7),60]);
  assert.equal(janky.slowFrames,1);
  assert.equal(janky.p95Ms,60);
  assert.equal(summarize([]),null);
  assert.match(html,/id="motionBenchmarkBtn"/);
  assert.match(html,/id="motionBenchmarkResult"/);
});

test('rapid native expand followed by collapse queues final Windows size',async()=>{
  const section=(a,b)=>{
    const from=app.indexOf(a),to=app.indexOf(b,from);
    assert.ok(from>=0&&to>from);
    return app.slice(from,to);
  };
  const native=section('let nativeResizeQueue=','const KEY=');
  const commit=section('function commitExpanded(','let expandRequest=');
  const toggle=section('async function setExpanded(','function showTab(');
  const classes=new Set(['collapsed']);
  const issued=[];
  let finishFirst;
  const gate=new Promise(resolve=>{finishFirst=resolve});
  const mocks={
    nativeShell:true,
    nativeApi:{core:{invoke:async(_cmd,args)=>{
      issued.push(args.expanded);
      if(issued.length===1)await gate;
      return null
    }}},
    island:{
      classList:{
        contains:name=>classes.has(name),
        add:name=>classes.add(name),
        remove:name=>classes.delete(name),
        toggle:(name,on)=>on?classes.add(name):classes.delete(name)
      },
      style:{setProperty(){}}
    },
    panel:{inert:true},
    motion:{
      motionAllowed:()=>true,
      timings:()=>({morph:560}),
      onceTransition:()=>Promise.resolve(),
      clearLyrics(){}
    },
    options:{reducedMotion:false},
    requestAnimationFrame:()=>1,
    resizeWave(){},drawWave(){},startWaveLoop(){},
    syncTabIndicator(){},refreshIslandHeight(){},clearWave(){},
    setTimeout:()=>1,clearTimeout(){},
    console:{warn(){}}
  };
  const expr=`
    let nativeBridgeStatus='';
    let expanded=false,morphSequence=0,morphTimer=0;
    ${native}
    ${commit}
    let expandRequest=0;
    ${toggle}
    return {setExpanded,getState:()=>({expanded,collapsed:island.classList.contains('collapsed')})}
  `;
  const simulator=runInNewContext(
    'new Function(...Object.keys(inputs),source)(...Object.values(inputs))',
    {inputs:mocks,source:expr}
  );
  const opening=simulator.setExpanded(true);
  for(let i=0;i<5;i++)await Promise.resolve();
  const closing=simulator.setExpanded(false);
  finishFirst();
  await Promise.all([opening,closing]);
  assert.deepEqual(issued,[true,false]);
  assert.equal(simulator.getState().expanded,false);
  assert.equal(simulator.getState().collapsed,true);
});

test('weather and music metadata change with optional appearance motion',()=>{
  assert.match(app,/function animateLabel\(/);
  assert.match(app,/animateLabel\('#trackName'/);
  assert.match(app,/animateLabel\('#weatherTemp'/);
  assert.match(code,/function animateValue\(/);
  const {motion}=engine(true);
  const label={textContent:'Before'};
  motion.animateValue(label,'After',{enabled:true});
  assert.equal(label.textContent,'After');
});

test('snow uses real frame delta and does not stay at old 22fps',()=>{
  assert.match(app,/frameSeconds=snowFrame\?/);
  assert.match(app,/flake\.v\*24\*frameSeconds\/h/);
  assert.doesNotMatch(app,/time-snowFrame>=44/);
});
