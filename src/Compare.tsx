import {useEffect,useRef,useState} from 'react';
import {ArrowLeft,ArrowRight,ArrowsClockwise,CircleNotch,Eye,WarningCircle,X} from '@phosphor-icons/react';
import {call,errorText,native,on,time} from './bridge';
import {diffLabel,pageLabel,routeOf,widths,widthsOf,type CompareIndex,type CompareStatus,type Width} from './comparison';
import type {Project} from './types';
import './compare.css';

type Notify=(message:string,error?:boolean)=>void;
/** How often a running comparison's status is read. */
const POLL_MS=2000;

/** Compare: on the owner's click the plugin captures the source beside the
 * preview WordPress, page by page, at 1440 and 390; the owner walks through
 * the pages. Only what the plugin measured is shown; nothing is approved. */
export function ComparePanel({project,running,notify}:{project:Project;running:boolean;notify:Notify}){
 const[found,setFound]=useState<CompareIndex|null>(null);
 const[status,setStatus]=useState<CompareStatus|null>(null);
 const[starting,setStarting]=useState(false);
 const[at,setAt]=useState(0);
 const[width,setWidth]=useState<Width>('desktop');
 const[image,setImage]=useState('');const[original,setOriginal]=useState('');
 const gutenberg=project.target==='h2g';const astro=project.target==='astro';
 const[zoom,setZoom]=useState(false);
 const watched=useRef(false);
 const pages=found?.pages||[];
 const page=pages[Math.min(at,pages.length-1)];
 const offered=widthsOf(pages);
 const shown=page?.[width];
 const sourceLabel=shown?.referenceKind==='astro-reference'?'Astro 5 reference':gutenberg?'Original HTML theme':'Original site';
 const convertedLabel=shown?.referenceKind==='astro-reference'?'WordPress conversion':gutenberg?'Gutenberg conversion':astro?'Astro 5 conversion':'WordPress conversion';
 const comparing=starting||!!status?.running;
 const load=()=>call<CompareIndex>('compare_index',{projectId:project.id}).then(setFound).catch(()=>setFound({}));
 const poll=()=>call<CompareStatus>('compare_status',{projectId:project.id}).then(setStatus).catch(()=>{});
 useEffect(()=>{if(!native)return;void load();void poll()},[project.id]);
 // While it runs: its note, every two seconds; at its end, the new index or why it stopped.
 useEffect(()=>{
  if(!native||!comparing)return;
  const timer=setInterval(()=>void poll(),POLL_MS);
  return()=>clearInterval(timer);
 },[project.id,comparing]);
 useEffect(()=>{
  const off=on<{projectId:string;error:string|null}>('compare-finished',v=>{
   if(v.projectId!==project.id)return;
   void poll();void load();
   if(watched.current)notify(v.error?v.error:'Comparison ready.',!!v.error);
   watched.current=false;
  });
  return()=>{void off.then(fn=>fn())};
 },[project.id]);
 useEffect(()=>{if(offered.length&&!offered.includes(width))setWidth(offered[0])},[offered.join(),width]);
 useEffect(()=>{
  setImage('');setOriginal('');if(!shown?.image)return;let alive=true;
  call<string>('compare_image',{projectId:project.id,path:shown.image}).then(v=>{if(alive)setImage(v)}).catch(e=>{if(alive)notify(errorText(e),true)});
  // Gutenberg from an HTML theme: the original page is its own image, shown beside the new one.
  if(shown.original)call<string>('compare_image',{projectId:project.id,path:shown.original}).then(v=>{if(alive)setOriginal(v)}).catch(e=>{if(alive)notify(errorText(e),true)});
  return()=>{alive=false};
 },[project.id,shown?.image,shown?.original,found?.capturedAt]);
 useEffect(()=>{if(!zoom)return;const close=(e:KeyboardEvent)=>{if(e.key==='Escape')setZoom(false)};document.addEventListener('keydown',close);return()=>document.removeEventListener('keydown',close)},[zoom]);
 async function generate(pageKey?:string){
  setStarting(true);watched.current=true;
  try{await call('compare_generate',{projectId:project.id,pageKey});await poll()}
  catch(e){watched.current=false;notify(errorText(e),true)}
  finally{setStarting(false)}
 }
 const blocked=running?'Available when the conversion finishes or is stopped.':null;
 const problem=!comparing&&(status?.error||(status?.state==='failed'?status.note:null));
 return <section className="compare-page">
  <div className="section-heading"><div><h2>Page by page</h2><p className="muted">{shown?.referenceKind==='astro-reference'?'The retained Astro build beside WordPress, at desktop and mobile width.':gutenberg?'Your original HTML theme beside the new Gutenberg theme, at desktop and mobile width. Refresh all pages or only the selected page.':astro?'Your original site beside the built Astro site, at desktop and mobile width. Refresh all pages or only the selected page.':'Your original site beside the WordPress preview, at desktop and mobile width. Refresh all pages or only the selected page.'}</p></div>
   {page&&<button className="button" type="button" disabled={comparing||!!blocked} onClick={()=>void generate(page.key)}><ArrowsClockwise/>Refresh selected page</button>}
   <button className={`button ${pages.length?'':'primary'}`} type="button" disabled={comparing||!!blocked} title={blocked||undefined} onClick={()=>void generate()}>{comparing?<CircleNotch className="spin"/>:pages.length?<ArrowsClockwise/>:<Eye/>}{comparing?'Comparing…':pages.length?'Regenerate all pages':'Generate comparison'}</button></div>
  {comparing?<p className="compare-status" role="status"><CircleNotch className="spin" size={14}/>{status?.state==='running'&&status.note?`Comparing: ${status.note}`:'Starting the comparison…'} About a minute for a dozen pages.</p>
   :found?.capturedAt?<p className="compare-status">Last refreshed {time(found.capturedAt)} · {pages.length} page{pages.length===1?'':'s'}</p>
   :blocked&&<p className="compare-status">{blocked}</p>}
  {problem&&<div className="inline-error"><WarningCircle size={20}/><div><strong>The comparison did not finish</strong><p>{problem}</p></div></div>}
  {!pages.length?!comparing&&<div className="empty"><div className="empty-icon"><Eye size={32}/></div><h2>No comparison yet</h2><p>{gutenberg?'Once the conversion has built its sandbox WordPress (its Verify step), choose Generate comparison. You then page through the original and the Gutenberg theme side by side.':'Once the conversion has installed your theme, choose Generate comparison. You then page through the source and WordPress side by side.'}</p></div>
   :page&&<>
    <div className="compare-nav">
     <button className="button" type="button" aria-label="Previous page" disabled={at===0} onClick={()=>setAt(at-1)}><ArrowLeft/></button>
     <select aria-label="Page" value={page.key} onChange={e=>setAt(Math.max(0,pages.findIndex(p=>p.key===e.target.value)))}>{pages.map(p=><option key={p.key} value={p.key}>{pageLabel(p,width)}</option>)}</select>
     <button className="button" type="button" aria-label="Next page" disabled={at>=pages.length-1} onClick={()=>setAt(at+1)}><ArrowRight/></button>
     <span className="position">{Math.min(at,pages.length-1)+1} of {pages.length}</span>
    </div>
    {shown?.referenceNotice&&<div className="notice">{shown.referenceNotice}</div>}
    <div className="compare-meta"><h3>{page.title||page.key}</h3>{routeOf(page)&&<code>{routeOf(page)}</code>}<span className="diff">{shown?.image?diffLabel(shown.diffPercent):'Not captured'}</span></div>
    {offered.length>1&&<div className="compare-widths" role="group" aria-label="Width">{widths.filter(w=>offered.includes(w.width)).map(w=><button key={w.width} type="button" aria-pressed={w.width===width} className={`button ${w.width===width?'selected':''}`} onClick={()=>setWidth(w.width)}>{w.label}</button>)}</div>}
    <div className={`comparison-frame ${width}`}>
     <div className="comparison-labels"><span>{sourceLabel}</span><span>{convertedLabel}</span></div>
     <div className="comparison">{!shown?.image?<p>{shown?.error||'This page was not captured at this width.'}</p>:shown.original?(image&&original?<div className="comparison-pair" onClick={()=>setZoom(true)}><figure><img src={original} alt={`Original: ${page.title||page.key}`}/></figure><figure><img src={image} alt={`Gutenberg: ${page.title||page.key}`}/></figure></div>:<p><CircleNotch className="spin"/></p>):image?<img src={image} alt={`${sourceLabel} and ${convertedLabel}, side by side: ${page.title||page.key}`} onClick={()=>setZoom(true)}/>:<p><CircleNotch className="spin"/></p>}</div>
    </div>
   </>}
  {zoom&&image&&<div className="image-modal" role="dialog" aria-modal="true" aria-label="Full page comparison"><button className="button" type="button" onClick={()=>setZoom(false)}><X/>Close comparison</button><div className="comparison-labels"><span>{sourceLabel}</span><span>{convertedLabel}</span></div>{shown?.original&&original?<div className="comparison-pair"><figure><img src={original} alt="Full page original"/></figure><figure><img src={image} alt="Full page Gutenberg"/></figure></div>:<img src={image} alt={`Full page ${sourceLabel} and ${convertedLabel} comparison`}/>}</div>}
 </section>;
}
