import {useEffect,useState} from 'react';
import {ArrowsClockwise,CircleNotch} from '@phosphor-icons/react';
import {call,errorText} from './bridge';
import type {ModelCatalog} from './types';

export function ModelPicker({accountKey,running,selected,onSelected}:{accountKey:string;running:boolean;selected:string;onSelected:(model:string)=>void}){
 const[catalog,setCatalog]=useState<ModelCatalog|null>(null);
 const[loading,setLoading]=useState(true);
 const[saving,setSaving]=useState(false);
 const[error,setError]=useState('');
 const[refresh,setRefresh]=useState(0);
 const[effort,setEffort]=useState('');
 useEffect(()=>{
  let alive=true;setLoading(true);setError('');setCatalog(null);
  call<ModelCatalog>('model_catalog').then(value=>{if(alive){setCatalog(value);setEffort(value.selectedEffort||'')}}).catch(e=>{if(alive)setError(errorText(e))}).finally(()=>{if(alive)setLoading(false)});
  return()=>{alive=false};
 },[accountKey,refresh]);
 const model=catalog?.models.find(m=>m.model===selected);
 const defaultModel=catalog?.models.find(m=>m.isDefault);
 const unavailable=!!selected&&!!catalog&&!model;
 const activeModel=model||(!selected?defaultModel:undefined);
 const efforts=activeModel?.supportedReasoningEfforts||[];
 const effortUnavailable=!!effort&&!efforts.some(option=>option.reasoningEffort===effort);
 const label=(value:string)=>value==='xhigh'?'Extra high':value.charAt(0).toUpperCase()+value.slice(1);
 async function choose(value:string){setSaving(true);setError('');try{const result=await call<{selectedEffort:string}>('select_model',{model:value});setEffort(result.selectedEffort||'');onSelected(value)}catch(e){setError(errorText(e))}finally{setSaving(false)}}
 async function chooseEffort(value:string){setSaving(true);setError('');try{await call('select_effort',{model:selected,effort:value});setEffort(value)}catch(e){setError(errorText(e))}finally{setSaving(false)}}
 return <div className="model-picker">
  <label htmlFor="conversion-model">Conversion model</label>
  <p>The default Codex model for new projects. Each project can use its own model from its chat, so several projects can convert at once on different models.</p>
  <div className="model-picker-controls">
   <select id="conversion-model" value={selected} disabled={running||loading||saving||!catalog} onChange={e=>void choose(e.target.value)} aria-describedby="model-help">
    <option value="">{defaultModel?`Codex default · ${defaultModel.displayName}`:'Codex default'}</option>
    {unavailable&&<option value={selected} disabled>{selected} · unavailable</option>}
    {catalog?.models.map(m=><option key={m.model} value={m.model}>{m.displayName}{m.isDefault?' · default':''}</option>)}
   </select>
   <button type="button" className="button" disabled={running||loading||saving} onClick={()=>setRefresh(v=>v+1)} aria-label="Refresh Codex models">{loading||saving?<CircleNotch className="spin"/>:<ArrowsClockwise/>}</button>
  </div>
  <p id="model-help" className="inline-note">{running?'You can change the model when the current conversation finishes.':loading?'Loading the model catalog from your connected Codex account…':saving?'Saving your model…':activeModel?.description||'Your choice is saved and applies to the next conversation turn, including resumed projects.'}</p>
  <div className="effort-picker">
   <label htmlFor="reasoning-effort">Reasoning effort</label>
   <div className="model-picker-controls">
    <select id="reasoning-effort" value={effort} disabled={running||loading||saving||!activeModel||efforts.length===0} onChange={e=>void chooseEffort(e.target.value)} aria-describedby="effort-help">
     <option value="">{activeModel?.defaultReasoningEffort?`Model default · ${label(activeModel.defaultReasoningEffort)}`:'Model default'}</option>
     {effortUnavailable&&<option value={effort} disabled>{label(effort)} · unavailable</option>}
     {efforts.map(option=><option key={option.reasoningEffort} value={option.reasoningEffort}>{label(option.reasoningEffort)}</option>)}
    </select>
   </div>
   <p id="effort-help" className="inline-note">{running?'You can change effort when the current conversation finishes.':efforts.find(option=>option.reasoningEffort===(effort||activeModel?.defaultReasoningEffort))?.description||'Saved separately for each model. Applies to the next conversion step or AI edit.'}</p>
   {effortUnavailable&&<p className="model-error" role="alert">Your saved effort is no longer supported. Choose a listed effort or Model default.</p>}
  </div>
  {unavailable&&<p className="model-error" role="alert">Choose another model or Codex default. Your saved model is no longer listed.</p>}
  {catalog?.models.length===0&&<p className="model-error" role="alert">Codex returned no models. Refresh the catalog before converting.</p>}
  {error&&<p className="model-error" role="alert">{error}</p>}
 </div>
}
