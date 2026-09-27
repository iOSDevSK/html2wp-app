import {useEffect,useState} from 'react';
import {save} from '@tauri-apps/plugin-dialog';
import {CircleNotch,Download,PencilSimple} from '@phosphor-icons/react';
import {call,errorText} from './bridge';

type Lite={available:boolean;version?:string};
/** The free editor for the converted theme, offered next to the theme ZIP. */
export function VisualEditLiteRow({disabled,notify}:{disabled:boolean;notify:(message:string,error?:boolean)=>void}){
 const[lite,setLite]=useState<Lite|null>(null);
 const[saving,setSaving]=useState(false);
 useEffect(()=>{let alive=true;call<Lite>('visual_edit_lite').then(v=>{if(alive)setLite(v)}).catch(()=>{if(alive)setLite({available:false})});return()=>{alive=false}},[]);
 async function download(){
  if(!lite?.version)return;
  const destination=await save({defaultPath:`visual-edit-lite-${lite.version}.zip`,filters:[{name:'WordPress plugin',extensions:['zip']}]});
  if(!destination)return;
  setSaving(true);
  try{await call('export_visual_edit_lite',{destination});notify('Visual Edit Lite saved. Install it in WordPress under Plugins → Add New → Upload Plugin.')}
  catch(e){notify(errorText(e),true)}
  finally{setSaving(false)}
 }
 return <div className="export-row">
  <span className="export-icon"><PencilSimple size={32} weight="duotone"/></span>
  <div><h3>Visual Edit Lite{lite?.version?` ${lite.version}`:''}</h3><p>Free editor for this theme (HTML and Gutenberg): edit text, images and sections by clicking the page. Install it in WordPress under Plugins → Add New → Upload Plugin.</p><small>{lite===null?'Checking the latest release…':lite.available?'Latest release from GitHub':'Not available offline yet'}</small></div>
  <button className="button" type="button" disabled={disabled||saving||!lite?.available} onClick={()=>void download()}>{saving||lite===null?<CircleNotch className="spin"/>:<Download/>}Download ZIP</button>
 </div>;
}
