import Swiper from './swiper.mjs';
export function createPages(){
 const root=document.getElementById('listen-pages'),panels=[document.getElementById('player'),document.getElementById('queue-section'),document.getElementById('scene-section')];let swiper;
 function selected(index){document.dispatchEvent(new CustomEvent('sda-page',{detail:index}));panels.forEach((panel,i)=>{panel.inert=i!==index;panel.setAttribute('aria-hidden',String(i!==index));});root.querySelectorAll('.page-dots span').forEach((dot,i)=>dot.classList.toggle('active',i===index));}
 const viewport=document.getElementById('listen-swiper');
 viewport.addEventListener('keydown',e=>{if(e.target!==viewport||!swiper)return;if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();e.key==='ArrowLeft'?swiper.slidePrev():swiper.slideNext();}});
 return {
 show(){root.hidden=false;if(!swiper)swiper=new Swiper('#listen-swiper',{autoHeight:true,spaceBetween:16,speed:matchMedia('(prefers-reduced-motion: reduce)').matches?0:280,threshold:12,touchAngle:30,noSwipingSelector:'#scene-host,input,select,a,summary,progress,.transport button,.choice-menu button,#player-settings,.sound-tools-launch button,.media-launch button',on:{slideChange:s=>selected(s.activeIndex)}});swiper.update();selected(swiper.activeIndex);},
 update(){requestAnimationFrame(()=>{swiper?.update();swiper?.updateAutoHeight();});},
 hide(){root.hidden=true;document.dispatchEvent(new CustomEvent('sda-page',{detail:-1}));}
 };
}
