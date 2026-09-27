import {useCallback,useEffect,useState} from 'react';
import {ArrowSquareOut,CircleNotch,Copy,Eye,Stop} from '@phosphor-icons/react';
import {call,errorText} from './bridge';
import {deployGate} from './cloudflare';
import type {Project} from './types';
import './cloudflare.css';

type Notify=(message:string,error?:boolean)=>void;
export type SitePreviewStatus={distReady:boolean;distReason:string|null;url:string|null};

/** Why the preview cannot start, or null when it can. */
export function sitePreviewBlocker(project:Project,running:boolean,status:SitePreviewStatus|null):string|null{
 const gate=deployGate(project,running);
 if(!gate.enabled)return gate.reason;
 return status&&!status.distReady?status.distReason:null;
}
/** Only this computer's own preview server may be opened or copied. */
export function isLocalPreviewUrl(url:string):boolean{return /^http:\/\/127\.0\.0\.1:\d{1,5}\/$/.test(url)}

/** Exports row: serve the built static site on this computer before deploying it. */
export function SitePreviewRow({project,running,notify}:{project:Project;running:boolean;notify:Notify}){
 const[status,setStatus]=useState<SitePreviewStatus|null>(null);const[starting,setStarting]=useState(false);
 const refresh=useCallback(()=>{let alive=true;call<SitePreviewStatus>('site_preview_status',{projectId:project.id}).then(v=>{if(alive)setStatus(v)}).catch(()=>{if(alive)setStatus(null)});return()=>{alive=false}},[project.id]);
 useEffect(()=>refresh(),[refresh,project.updatedAt]);
 const url=status?.url&&isLocalPreviewUrl(status.url)?status.url:null;
 const blocker=sitePreviewBlocker(project,running,status);
 async function start(){
  setStarting(true);
  try{const v=await call<{url:string}>('site_preview_start',{projectId:project.id});setStatus(s=>({distReady:true,distReason:null,...s,url:v.url}));await call('open_url',{url:v.url})}
  catch(e){notify(errorText(e),true)}
  finally{setStarting(false)}
 }
 async function stop(){try{await call('site_preview_stop');setStatus(s=>s&&{...s,url:null})}catch(e){notify(errorText(e),true)}}
 async function copy(){if(!url)return;try{await navigator.clipboard.writeText(url);notify('Copied to clipboard.')}catch{notify('Clipboard is unavailable. Select and copy the address.',true)}}
 return <div className="export-row site-preview-row">
  <span className="export-icon"><Eye size={32} weight="duotone"/></span>
  <div><h3>Astro 5 site preview {url&&<span className="status success"><span className="status-dot"/>Running</span>}</h3>
   {url?<div className="cf-url"><code>{url}</code><button className="button" type="button" onClick={()=>void call('open_url',{url}).catch(e=>notify(errorText(e),true))}><ArrowSquareOut/>Open</button><button className="button" type="button" onClick={()=>void copy()}><Copy/>Copy</button><button className="button" type="button" onClick={()=>void stop()}><Stop/>Stop preview</button></div>
    :<p>Click through the static site built during the conversion, exactly as it will be deployed. It runs only on this computer.</p>}
   {!url&&blocker&&<small>{blocker}</small>}
  </div>
  {!url&&<button className="button" type="button" disabled={!!blocker||starting} title={blocker||undefined} onClick={()=>void start()}>{starting?<CircleNotch className="spin"/>:<Eye/>}{starting?'Starting…':'Preview Astro 5 site'}</button>}
 </div>;
}
