import type {Artifact} from './types';

type Kind=Artifact['kind'];
/** A revision's files in display order: the theme first, then the report, then the rest as delivered. */
export function exportOrder(files:Artifact[]):Artifact[]{
 const rank:Record<Kind,number>={theme:0,pdf:1,report:2,summary:2,astro:3,editor:4,file:5};
 return [...files].sort((a,b)=>rank[a.kind]-rank[b.kind]);
}
export const exportTitle:Record<Kind,string>={theme:'WordPress theme',pdf:'Conversion report (PDF)',report:'Conversion report',astro:'Astro 5 project',editor:'Visual Edit plugin',summary:'Verification summary',file:'Delivered file'};
/** Save dialog filter for a delivered file: its own extension. */
export function exportFilter(a:Pick<Artifact,'kind'|'filename'>){
 const extension=a.filename.includes('.')?a.filename.split('.').pop()!.toLowerCase():'';
 const name=a.kind==='pdf'?'PDF report':a.kind==='theme'?'WordPress theme':a.kind==='report'?'Conversion report':a.kind==='summary'?'Verification summary':a.kind==='astro'?'Astro 5 project':a.kind==='editor'?'WordPress plugin':'File';
 return {name,extensions:extension?[extension]:['*']};
}
/** Which run made a file, for its row. */
export function runLabel(a:Pick<Artifact,'checks'>):string{
 return a.checks==='flash'?'Flash conversion':a.checks==='full'?'Full conversion':a.checks==='h2g'?'Gutenberg from an HTML theme':a.checks==='packaged'?'Packaged after changes, not checked again':'Earlier release';
}
