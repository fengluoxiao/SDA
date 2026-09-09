import {Children,isValidElement,useEffect,useId,useLayoutEffect,useRef,useState} from "react";
import type {SelectHTMLAttributes,ReactNode} from "react";
import {createPortal} from "react-dom";
import {Check,ChevronDown} from "lucide-react";

// Keep native change events and option semantics for all existing settings consumers.
export default function Select(props:SelectHTMLAttributes<HTMLSelectElement>) {
  const {children,disabled,title,className,...rest}=props;
  const native=useRef<HTMLSelectElement>(null),trigger=useRef<HTMLButtonElement>(null),menu=useRef<HTMLDivElement>(null);
  const [open,setOpen]=useState(false),[active,setActive]=useState(0),[position,setPosition]=useState({left:0,top:0,width:200,maxHeight:280});
  const id=useId(),search=useRef({text:"",at:0});
  const options=Children.toArray(children).filter(isValidElement).map(child=>{
    const p=child.props as {value?:string;children?:ReactNode;disabled?:boolean};
    const text=(v:ReactNode):string=>Children.toArray(v).map(n=>isValidElement(n)?text((n.props as {children:ReactNode}).children):String(n)).join("");
    return {value:String(p.value??text(p.children)),label:text(p.children),disabled:!!p.disabled};
  });
  const selected=options.findIndex(o=>o.value===String(props.value??props.defaultValue??""));
  const close=()=>{setOpen(false);trigger.current?.focus();};
  const choose=(index:number)=>{
    const option=options[index];if(!option||option.disabled||!native.current)return;
    native.current.value=option.value;
    native.current.dispatchEvent(new Event("change",{bubbles:true}));
    close();
  };
  const show=()=>{setActive(selected>=0&&!options[selected]?.disabled?selected:options.findIndex(o=>!o.disabled));setOpen(true);};
  useLayoutEffect(()=>{
    if(!open)return;
    const place=()=>{const r=trigger.current!.getBoundingClientRect();const below=innerHeight-r.bottom-12,above=r.top-12;
      const height=Math.min(320,Math.max(below,above));const up=below<180&&above>below;
      const width=Math.min(innerWidth-16,Math.max(r.width,360));
      setPosition({left:Math.max(8,Math.min(r.left,innerWidth-width-8)),
        top:up?Math.max(8,r.top-Math.min(height,options.length*44+8)-4):r.bottom+4,
        width,maxHeight:Math.max(80,up?above:below)});};
    place();window.addEventListener("resize",place);window.addEventListener("scroll",place,true);
    const outside=(e:PointerEvent)=>{if(!trigger.current?.contains(e.target as Node)&&!menu.current?.contains(e.target as Node))setOpen(false);};
    document.addEventListener("pointerdown",outside);
    return()=>{window.removeEventListener("resize",place);window.removeEventListener("scroll",place,true);document.removeEventListener("pointerdown",outside);};
  },[open,options.length]);
  useEffect(()=>{menu.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({block:"nearest"});},[active,open]);
  useEffect(()=>{if(disabled)setOpen(false);},[disabled]);
  return <>
    <select {...rest} ref={native} disabled={disabled} hidden aria-hidden="true" tabIndex={-1}>{children}</select>
    <button type="button" ref={trigger} className={`sda-select ${className??""}`} role="combobox" aria-label={props["aria-label"]??title??options[selected]?.label} aria-expanded={open} aria-controls={open?id:undefined} aria-haspopup="listbox" aria-activedescendant={open&&active>=0?`${id}-${active}`:undefined} disabled={disabled} title={title??options[selected]?.label}
      onClick={()=>open?close():show()} onKeyDown={e=>{
        if(e.key==="Escape"&&open){e.preventDefault();e.stopPropagation();close();return;}
        if(e.key==="Tab"){setOpen(false);return;}
        if(["ArrowDown","ArrowUp","Home","End"].includes(e.key)){
          e.preventDefault();if(!open){show();return;}
          const valid=options.map((o,i)=>o.disabled?-1:i).filter(i=>i>=0),at=valid.indexOf(active);
          setActive((e.key==="Home"?valid[0]:e.key==="End"?valid.at(-1):valid[(at+(e.key==="ArrowDown"?1:-1)+valid.length)%valid.length])??-1);return;
        }
        if(e.key==="Enter"||e.key===" "){e.preventDefault();open?choose(active):show();return;}
        if(e.key.length===1&&!e.ctrlKey&&!e.metaKey){const now=Date.now();search.current={text:(now-search.current.at>700?"":search.current.text)+e.key.toLowerCase(),at:now};const found=options.findIndex(o=>!o.disabled&&o.label.toLowerCase().startsWith(search.current.text));if(found>=0){setActive(found);setOpen(true);}}
      }}><span>{options[selected]?.label??"选择"}</span><ChevronDown size={14}/></button>
    {open&&createPortal(<div ref={menu} id={id} role="listbox" className="sda-select-menu" style={position} onMouseDown={e=>e.preventDefault()}>
      {options.map((o,i)=><div key={o.value} id={`${id}-${i}`} data-index={i} role="option" aria-selected={i===selected} aria-disabled={o.disabled} className={`sda-select-option ${i===active?"active":""}`} onPointerMove={()=>!o.disabled&&setActive(i)} onClick={()=>choose(i)}><span>{o.label}</span>{i===selected&&<Check size={15}/>}</div>)}
    </div>,document.body)}
  </>;
}
