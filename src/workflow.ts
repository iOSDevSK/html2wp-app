import type {Project,ThemeTarget} from './types';

export const themeTargets:{value:ThemeTarget;label:string;short:string;detail:string;experimental?:boolean;retired?:boolean}[]=[
 {value:'html',label:'HTML WordPress theme',short:'HTML',detail:'Default. Pixel-faithful theme, editable with Visual Edit.'},
 // Earlier releases' projects keep their label; a Gutenberg theme is now made from the HTML theme (the h2g card).
 {value:'gutenberg',label:'Gutenberg block theme',short:'Gutenberg',detail:'Native WordPress blocks, editable in the block editor.',experimental:true,retired:true},
 {value:'astro',label:'Astro 5 project only',short:'Astro 5',detail:'Just the Astro 5 project and its built site, no WordPress. No service conversion.'},
 {value:'h2g',label:'Gutenberg from an HTML theme',short:'Gutenberg from HTML theme',detail:'Turns an HTML WordPress theme made by html2wp into a native block theme, editable in the block editor. Usually takes 3–5 hours.'},
];
/** Projects saved before theme types existed are HTML themes. */
export function projectTarget(project:Pick<Project,'target'>):ThemeTarget{return project.target==='gutenberg'||project.target==='astro'||project.target==='h2g'?project.target:'html'}
/** The html2wp-to-gutenberg workflow's steps (its docs/workflow.md), as the Overview shows them. */
export const h2gStages:[string,string][]=[['Audit','The theme and its content, inventoried'],['Scaffold and theme.json','The new block theme and its design tokens'],['Fonts','The exact font files, local'],['CSS','The design carried over, bridged to core blocks'],['JS','The site\'s behaviour kept'],['Parts, templates and patterns','Header, footer, menus and page shells'],['Content','Every page and post as core blocks'],['Importer and setup','The one-click content import'],['Verify','A real WordPress, compared with the original']];
export type H2gProgress={current?:number;started?:string;steps?:Record<string,{at:string;note:string}>;changes?:ThemeChanges|null};
/** Where the run is: the step the agent reported (0-based), -1 before it starts, past the end once delivered. */
export function h2gStage(project:Pick<Project,'phase'>,progress:H2gProgress):number{return project.phase==='deliverable_ready'?h2gStages.length:progress.current?progress.current-1:-1}
/** How long it has been running, next to how long it usually takes. */
export function h2gElapsed(progress:H2gProgress,now:number):string|null{
 if(!progress.started)return null;
 return `${duration(progress.started,now)} so far · usually 3–5 hours`;
}
function duration(since:string,now:number):string{
 const minutes=Math.max(0,Math.floor((now-Date.parse(since))/60000));
 return minutes>=60?`${Math.floor(minutes/60)} h ${minutes%60} min`:`${minutes} min`;
}
/** "Still working": when the run last did something. */
export function lastActivity(at:string|undefined,now:number):string|null{
 if(!at)return null;
 const minutes=Math.max(0,Math.floor((now-Date.parse(at))/60000));
 return minutes<1?'last activity just now':`last activity ${minutes} min ago`;
}
/** Gutenberg from an HTML theme: the input is a theme html2wp made, not a site. */
export function fromTheme(target:ThemeTarget):boolean{return target==='h2g'}
/** The types a project can still switch to: the input decides between a site's types and a theme's. */
export function switchableTargets(target:ThemeTarget):ThemeTarget[]{return themeTargets.filter(t=>!t.retired).map(t=>t.value).filter(t=>fromTheme(t)===fromTheme(target))}
/** The types a new project can choose. */
export const offeredTargets:ThemeTarget[]=themeTargets.filter(t=>!t.retired).map(t=>t.value);
export function targetLabel(project:Pick<Project,'target'>):string{return themeTargets.find(t=>t.value===projectTarget(project))!.label}
/** Gutenberg block themes are experimental in this version. */
export const EXPERIMENTAL_NOTE='Experimental in this version: full Gutenberg support comes next.';
/** A theme type as the owner sees it, marked when it is experimental. */
export function targetName(target:ThemeTarget,compact=false):string{const t=themeTargets.find(x=>x.value===target)!;return `${compact?t.short:t.label}${t.experimental?' (experimental)':''}`}
export function experimental(target:ThemeTarget):boolean{return themeTargets.some(t=>t.value===target&&t.experimental)}
/** Mirrors the native rule: the type is fixed once work has started. */
export function canChangeTarget(project:Project):boolean{return project.phase==='imported'&&!project.lastStep&&!project.threadId}

/** A site's conversion, as the html2wp plugin reports it: {workspace}/progress.json (h2wp-progress/1, written by its progress.sh). */
export type SkillStage={stage:string;label:string;percent?:number;state?:'pending'|'running'|'done'|'warned'|'skipped'|'failed';note?:string};
export type SkillProgress={repairs?:Repair[];schema?:'h2wp-progress/1';mode?:'flash'|'full';target?:string|null;stage?:string|null;label?:string;percent?:number;state?:'running'|'finished'|'stopped';note?:string;next?:string;updatedAt?:string;stages?:SkillStage[]};
/** The plugin's verdict once the run ended ({output}/result.json, h2wp-result/1), as far as the Overview shows it. */
export type SkillResult={recovery?:{available?:boolean;stages?:string[];action?:string};status?:'delivered'|'stopped'|null;verdict?:string|null;stopped?:{stage?:string;reason?:string}|null;repairs?:Repair[]|null;couldNotFix?:Unfixed[]|null};
export type StageState='done'|'current'|'warned'|'failed'|'waiting'|'skipped';
/** The Overview's rows: the plugin's own stages, each with its note once it wrote one. */
export function skillStages(progress:SkillProgress):{label:string;detail:string;state:StageState}[]{
 const stages=(progress.stages||[]).filter(s=>s&&typeof s.label==='string');
 const state=(s:SkillStage):StageState=>s.state==='done'?'done':s.state==='warned'?'warned':s.state==='failed'?'failed':s.state==='skipped'?'skipped':s.state==='running'?'current':'waiting';
 return stages.map(s=>({label:s.label,detail:s.note||'',state:state(s)}));
}
/** The run in one line: its mode, how far it is by the plugin's own table, and what comes next. */
export function skillSummary(progress:SkillProgress):string|null{
 if(!progress.schema)return null;
 const mode=progress.mode==='flash'?'Flash conversion':'Full conversion';
 if(progress.state==='finished')return `${mode} · finished`;
 if(progress.state==='stopped')return `${mode} · stopped${progress.label?` at ${progress.label}`:''}`;
 return [mode,typeof progress.percent==='number'?`${progress.percent}%`:null,progress.next&&progress.next!=='—'?`next: ${progress.next}`:null].filter(Boolean).join(' · ');
}

export type RunMode='flash'|'full'|'astro';
/** The plugin's runs: Flash and Full make an HTML theme, the Astro run the Astro 5 project. Each is a goal text of the same skill. */
export const runModes:{mode:RunMode;label:string;detail:string}[]=[
 {mode:'flash',label:'Flash',detail:'The fast conversion: the AI runs every stage once, no repair loops; red checks go into the report.'},
 {mode:'full',label:'Full',detail:'Every check, bounded repairs, and a theme ZIP with remaining issues reported.'},
 {mode:'astro',label:'Build Astro project',detail:'The Astro 5 project and its built site, stage by stage, once; no WordPress theme and no service conversion.'},
];
/** The runs a project's output takes. */
export function runsFor(target:ThemeTarget):RunMode[]{return target==='astro'?['astro']:target==='html'?['flash','full']:[]}
/** A run that started and has not delivered: Continue resumes it. */
export function unfinishedRun(project:Pick<Project,'phase'|'threadId'>):boolean{return !!project.threadId&&project.phase!=='imported'&&project.phase!=='deliverable_ready'}
/** What the project heading offers when nothing runs. A delivered site changes in its live preview; a new run is "Start over from the original" in the project menu. */
export function runChoices(project:Pick<Project,'target'|'phase'|'threadId'>,stopped=false):{primary:'start'|'continue'|RunMode;secondary:RunMode[]}|null{
 const target=projectTarget(project);
 // A delivered Gutenberg theme changes in its sandbox; its conversion is not started again from here.
 if(target==='h2g')return project.phase==='deliverable_ready'?null:{primary:project.phase==='imported'?'start':'continue',secondary:[]};
 const modes=runsFor(target);
 // An earlier release's Gutenberg project has no run here: Clean & restart chooses another output.
 if(!modes.length||project.phase==='deliverable_ready')return null;
 // A run the plugin stopped: Continue repairs it (the repair-stop goal); a new run is "Start over", in the project menu.
 if(stopped)return {primary:'continue',secondary:[]};
 return unfinishedRun(project)?{primary:'continue',secondary:modes}:{primary:modes[0],secondary:modes.slice(1)};
}
/** The runs "Start over from the original" offers (the project menu, for when something broke): a delivered site converted again from its source, as a new run (--new). */
export function rebuildModes(project:Pick<Project,'target'|'phase'>,stopped=false):RunMode[]{return project.phase==='deliverable_ready'||stopped?runsFor(projectTarget(project)):[]}
/** Preserve an HTML run preference only when that mode is offered for this output. */
export function defaultRebuildMode(project:Pick<Project,'target'|'phase'|'flash'>,stopped=false):RunMode|null{
 const modes=rebuildModes(project,stopped);
 return project.flash===false&&modes.includes('full')?'full':modes[0]??null;
}

/** Changes after delivery (the plugin's workspace/changes.json, h2wp-changes/1): its own count since the last ZIP, whether the live theme differs from that ZIP, and every applied change since delivery. */
export type ThemeChanges={count:number|null;sinceZip:number|null;changedSinceZip:boolean};
/** The "Start over from the original" warning (the owner's words): what a new run from the source replaces. */
export function startOverNote(changes:ThemeChanges,stopped=false):string{
 if(stopped)return 'This converts the site again from its original source, as a new run. The stopped run and the repairs it allows are set aside. Use this only if something is broken; Continue repairs the stopped stage instead.';
 const base=`This converts the site again from its original source. All changes made after delivery (${changes.count}) are lost and the current theme is replaced. Use this only if something is broken.`;
 if(!changes.changedSinceZip)return base;
 const n=changes.sinceZip??0;
 return `${base} ${n===1?'The last change is':n>1?`The last ${n} changes are`:'The current changes are'} in no ZIP yet: use Make release first to keep ${n===1?'it':'them'} in a ZIP.`;
}
/** A turn on a delivered site is a change to its live preview, not a conversion. */
export function changeTurn(project:Pick<Project,'target'|'phase'>):boolean{return project.phase==='deliverable_ready'&&['html','h2g','astro'].includes(projectTarget(project))}
/** What is not in a release yet, in words: the plugin's count, or (a Gutenberg theme, no change log) that the theme differs from the last release. */
export function pendingRelease(changes:ThemeChanges):string|null{
 if(typeof changes.sinceZip==='number'&&changes.sinceZip>0)return `${changes.sinceZip} ${changes.sinceZip===1?'change':'changes'} since the last release.`;
 return changes.changedSinceZip?'The theme changed since the last release.':null;
}

/** A self-repair attempt of the plugin (progress.json/result.json `repairs`, APP-CONTRACT v1.4). */
export type Repair={stage?:string;attempt?:number;of?:number;lever?:string;label?:string;signature?:string;what?:string;outcome?:'open'|'fixed'|'failed';at?:string;note?:string;by?:'run'|'owner'};
/** What the run could not fix (result.json `couldNotFix`). */
export type Unfixed={stage?:string;signature?:string;what?:string;levers?:string[]};
/** The Repairs table's rows, as the plugin wrote them: the live progress while it runs, the result once it ended. */
export function repairRows(progress:{repairs?:Repair[]},result:{repairs?:Repair[]|null}):{stage:string;attempt:string;lever:string;outcome:string;by:string}[]{
 const rows=(progress.repairs?.length?progress.repairs:result.repairs)||[];
 return rows.filter(r=>r&&typeof r==='object').map(r=>({stage:String(r.stage??'—'),attempt:typeof r.attempt==='number'?`${r.attempt}/${r.of??'?'}`:'—',lever:r.label||r.lever||'—',
  outcome:r.outcome==='fixed'?'Fixed':r.outcome==='failed'?'Not fixed':'Repairing…',by:r.by==='owner'?'Your repair':'In the run'}));
}
