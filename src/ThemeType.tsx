import type {ThemeTarget} from './types';
import {EXPERIMENTAL_NOTE,offeredTargets,targetName,themeTargets} from './workflow';

/** HTML (default) or Gutenberg, or Gutenberg from an HTML theme: chosen at import, changeable until conversion starts. */
export function ThemeTypeChoice({value,onChange,disabled=false,compact=false,only}:{value:ThemeTarget;onChange:(target:ThemeTarget)=>void;disabled?:boolean;compact?:boolean;only?:ThemeTarget[]}){
 return <fieldset className={compact?'theme-type compact':'theme-type'} disabled={disabled} aria-label={compact?'Output type':undefined}>
  <legend>Output</legend>
  {themeTargets.filter(t=>(only||offeredTargets).includes(t.value)).map(t=><label key={t.value} className={value===t.value?'selected':''} title={compact?`${targetName(t.value)}${t.experimental?`. ${EXPERIMENTAL_NOTE}`:''}`:undefined}>
   <input type="radio" name={compact?'theme-type-project':'theme-type-import'} value={t.value} checked={value===t.value} onChange={()=>onChange(t.value)}/>
   <span><strong>{targetName(t.value,compact)}</strong>{!compact&&<small>{t.detail}</small>}{!compact&&t.experimental&&<small className="experimental-note">{EXPERIMENTAL_NOTE}</small>}</span>
  </label>)}
 </fieldset>;
}
