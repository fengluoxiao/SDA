import {animate,motionValue} from './motion.mjs';

export function installMoreMenu(root){
  const trigger=root.querySelector('summary'),panel=root.querySelector('.player-settings-menu');
  const reduced=matchMedia('(prefers-reduced-motion: reduce)');
  const backdrop=document.createElement('div');backdrop.className='more-menu-backdrop';backdrop.hidden=true;
  backdrop.setAttribute('aria-hidden','true');root.before(backdrop);
  const content=document.createElement('div');content.className='more-menu-content';
  while(panel.firstChild)content.append(panel.firstChild);panel.append(content);
  const progress=motionValue(0);
  let wanted=false,animation=null,revision=0,bounds=null;
  panel.id='more-menu-panel';trigger.setAttribute('aria-controls',panel.id);trigger.setAttribute('aria-expanded','false');
  function paint(value){
    if(!bounds)return;
    const p=Math.max(0,Math.min(1,value)),{width,height,diameter,dx,dy}=bounds;
    // Reference recording: a tall lens opens before its capsule corners relax.
    // Transform a complete menu, not a growing box that clips stationary text.
    // Preserve the spring's overshoot: clamping both axes to 1 made the
    // previous spring stop abruptly just when its elastic motion should show.
    const squash=Math.min(.14,Math.max(0,value-1));
    const sx=diameter/width+(1-diameter/width)*Math.pow(p,1.15)+squash*.65;
    const sy=diameter/height+(1-diameter/height)*Math.pow(p,.65)-squash*.4;
    // Keep the young lens near its source; the previous .35 exponent moved
    // its centre most of the way across while the shell was still tiny.
    const travel=1-Math.pow(p,.65);
    const corner=Math.max(0,Math.min(1,(p-.65)/.35));
    const relaxed=corner*corner*(3-2*corner);
    const radius=width*sx*.5*(1-relaxed)+26*relaxed;
    panel.style.transform=`translate(${dx*travel}px,${dy*travel}px) scale(${sx},${sy})`;
    panel.style.borderRadius=`${radius/sx}px / ${radius/sy}px`;
    panel.style.opacity=String(Math.min(1,p*5));
    panel.style.pointerEvents=wanted&&p>.85?'auto':'none';
    // The source button and menu exchange size through the same progress.
    // Reversing mid-flight therefore reverses both without a visibility jump.
    const buttonProgress=1-p;
    trigger.style.transform=`scale(${buttonProgress})`;
    trigger.style.opacity=String(Math.min(1,buttonProgress*2));
    // Across the recorded cycles, the glass leads and the lettering follows.
    // Use the same curve backwards: lettering clears early on collapse,
    // leaving the capsule to flow back into the growing source button.
    const reveal=Math.max(0,Math.min(1,(p-.60)/.38));
    const clarity=reveal*reveal*(3-2*reveal);
    content.style.opacity=String(clarity);
    content.style.filter=`blur(${6*(1-clarity)}px)`;
  }
  progress.on('change',paint);
  function settle(open){
    backdrop.hidden=!open;
    root.open=open;delete root.dataset.morphing;root.classList.toggle('more-expanded',open);
    for(const key of ['width','height','borderRadius','overflow','transform','transformOrigin','opacity','pointerEvents'])panel.style[key]='';
    for(const key of ['width','opacity','transform','filter'])content.style[key]='';
    trigger.style.transform='';trigger.style.opacity='';
    bounds=null;animation=null;panel.dispatchEvent(new Event('glass-refresh'));
  }
  function setOpen(open){
    if(open===wanted)return;
    wanted=open;const current=++revision,velocity=progress.getVelocity();
    animation?.stop();
    backdrop.hidden=false;
    root.classList.add('more-expanded');
    if(!open&&panel.contains(document.activeElement))trigger.focus();
    trigger.setAttribute('aria-expanded',String(open));panel.inert=!open;
    if(reduced.matches){progress.jump(open?1:0);settle(open);return;}
    root.open=true;
    if(!bounds){
      const a=trigger.getBoundingClientRect(),b=panel.getBoundingClientRect(),style=getComputedStyle(panel);
      bounds={width:b.width,height:b.height,diameter:a.width,dx:a.left+a.width/2-b.left-b.width/2,dy:a.top+a.height/2-b.top-b.height/2};
      content.style.width=`${b.width-parseFloat(style.paddingLeft)-parseFloat(style.paddingRight)}px`;
    }
    panel.style.transformOrigin='50% 50%';panel.style.overflow='hidden';root.dataset.morphing='true';paint(progress.get());
    // Motion preserves position and carries velocity into an interrupted spring.
    animation=animate(progress,open?1:0,{type:'spring',stiffness:open?170:240,damping:open?19:25,mass:1.05,
      velocity,restDelta:.001,restSpeed:.01,onComplete:()=>{if(current===revision)settle(open);}});
  }
  trigger.addEventListener('click',event=>{event.preventDefault();setOpen(!wanted);});
  root.addEventListener('menu-close',()=>setOpen(false));
  // Close on click, not pointerdown: even with reduced motion, the complete
  // gesture targets the shield instead of releasing onto a control below.
  backdrop.addEventListener('click',event=>{event.preventDefault();event.stopPropagation();setOpen(false);});
  backdrop.addEventListener('pointerdown',event=>event.stopPropagation());
  root.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();setOpen(false);trigger.focus();}});
  reduced.addEventListener('change',()=>{if(reduced.matches&&animation){revision++;animation.stop();progress.jump(wanted?1:0);settle(wanted);}});
  return {setOpen};
}
