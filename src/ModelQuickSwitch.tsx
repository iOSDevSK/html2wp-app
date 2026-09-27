import {useEffect,useState} from 'react';
import {call,errorText,on} from './bridge';
import type {ModelCatalog} from './types';

/** This project's model, beside its chat. Each project keeps its own model and
 * effort, so two projects can run at once on different models. Settings →
 * Conversion model is only the default for projects that have not chosen one. */
export function ModelQuickSwitch({projectId,selected,running,onSelected,notify}:{projectId:string;selected:string;running:boolean;onSelected:(model:string)=>void;notify:(message:string,error?:boolean)=>void}){
 const[catalog,setCatalog]=useState<ModelCatalog|null>(null);
 const[effort,setEffort]=useState('');
 const[saving,setSaving]=useState(false);
 const[rev,setRev]=useState(0);useEffect(()=>{const off=on('codex-updated',()=>setRev(r=>r+1));return()=>{void off.then(f=>f())}},[]);
 useEffect(()=>{let alive=true;call<ModelCatalog>('model_catalog',{projectId}).then(v=>{if(alive){setCatalog(v);setEffort(v.selectedEffort||'')}}).catch(()=>{});return()=>{alive=false}},[selected,projectId,rev]);
 if(!catalog||catalog.models.length===0)return null;
 const active=catalog.models.find(m=>m.model===selected)||catalog.models.find(m=>m.isDefault);
 const efforts=active?.supportedReasoningEfforts||[];
 const label=(v:string)=>v==='xhigh'?'Extra high':v.charAt(0).toUpperCase()+v.slice(1);
 async function choose(model:string){setSaving(true);try{const r=await call<{selectedModel:string;selectedEffort:string}>('select_model',{model,projectId});setEffort(r.selectedEffort||'');onSelected(r.selectedModel||model)}catch(e){notify(errorText(e),true)}finally{setSaving(false)}}
 async function chooseEffort(value:string){setSaving(true);try{await call('select_effort',{model:selected||catalog?.models.find(m=>m.isDefault)?.model||'',effort:value,projectId});setEffort(value)}catch(e){notify(errorText(e),true)}finally{setSaving(false)}}
 return <div className="chat-model" title={running?'You can change this project\'s model when its assistant finishes':'Model for this project'}>
  <select aria-label="Model for this project" value={selected} disabled={saving||running} onChange={e=>void choose(e.target.value)}>
   <option value="">{catalog.models.find(m=>m.isDefault)?.displayName||'Codex default'}</option>
   {catalog.models.filter(m=>!m.isDefault).map(m=><option key={m.model} value={m.model}>{m.displayName}</option>)}
  </select>
  {efforts.length>0&&<select aria-label="Reasoning effort for this project" value={effort} disabled={saving||running} onChange={e=>void chooseEffort(e.target.value)}>
   <option value="">{active?.defaultReasoningEffort?`${label(active.defaultReasoningEffort)} effort`:'Default effort'}</option>
   {efforts.map(o=><option key={o.reasoningEffort} value={o.reasoningEffort}>{label(o.reasoningEffort)} effort</option>)}
  </select>}
 </div>;
}
