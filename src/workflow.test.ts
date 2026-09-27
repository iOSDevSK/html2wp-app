import {describe,it,expect} from 'vitest';
import progressRunning from '../tests/fixtures/progress-running.json';
import progressFinished from '../tests/fixtures/progress-finished.json';
import progressStopped from '../tests/fixtures/progress-stopped.json';
import {changeTurn,offeredTargets,pendingRelease,repairRows,rebuildModes,defaultRebuildMode,startOverNote,runChoices,runModes,runsFor,skillStages,skillSummary,unfinishedRun,type SkillProgress} from './workflow';
import type {Project} from './types';

/** The plugin's own progress files, written by its progress.sh (tests/fixtures). */
const fixtures:Record<string,unknown>={'progress-running':progressRunning,'progress-finished':progressFinished,'progress-stopped':progressStopped};
const fixture=(name:string)=>fixtures[name] as SkillProgress;
const project=(phase:string,threadId:string|null=null,target?:Project['target'])=>({phase,threadId,target} as Project);
describe('converting a site: Flash and Full',()=>{
 it('restarts Astro with its own mode even when flash is false, while preserving HTML preference',()=>{
  for(const flash of [false,true,undefined]){
   expect(defaultRebuildMode({...project('deliverable_ready','t','astro'),flash})).toBe('astro');
   expect(defaultRebuildMode({...project('failed','t','astro'),flash},true)).toBe('astro');
  }
  expect(defaultRebuildMode({...project('deliverable_ready','t','html'),flash:false})).toBe('full');
  expect(defaultRebuildMode({...project('deliverable_ready','t','html'),flash:true})).toBe('flash');
  expect(defaultRebuildMode({...project('interrupted','t','astro'),flash:false})).toBeNull();
 });
 it('offers Flash first and Full beside it for an HTML theme, Continue first for a run that stopped',()=>{
  expect(runModes.map(m=>m.mode)).toEqual(['flash','full','astro']);
  expect(runChoices(project('imported'))).toEqual({primary:'flash',secondary:['full']});
  // Delivered: changes go to the live preview; a new run is "Start over from the original" in the project menu, behind its warning.
  expect(runChoices(project('deliverable_ready','t'))).toBeNull();
  expect(rebuildModes(project('deliverable_ready','t'))).toEqual(['flash','full']);
  expect(rebuildModes(project('deliverable_ready','t','astro'))).toEqual(['astro']);
  expect(rebuildModes(project('interrupted','t'))).toEqual([]);
  for(const phase of ['running','interrupted','failed'])expect(runChoices(project(phase,'t'))).toEqual({primary:'continue',secondary:['flash','full']});
  // Nothing to continue without a conversation.
  expect(unfinishedRun(project('failed'))).toBe(false);
 });
 it('builds an Astro 5 project with its own run, and offers no Gutenberg target (the h2g card makes Gutenberg)',()=>{
  expect(runsFor('html')).toEqual(['flash','full']);
  expect(runsFor('astro')).toEqual(['astro']);
  expect(runChoices(project('imported',null,'astro'))).toEqual({primary:'astro',secondary:[]});
  expect(runChoices(project('interrupted','t','astro'))).toEqual({primary:'continue',secondary:['astro']});
  expect(runModes.find(m=>m.mode==='astro')?.label).toBe('Build Astro project');
  expect(offeredTargets).toEqual(['html','astro','h2g']);
  // An earlier release's Gutenberg project: nothing to run here.
  expect(runChoices(project('interrupted','t','gutenberg'))).toBeNull();
 });
 it('Gutenberg from an HTML theme keeps its own Start and Continue',()=>{
  expect(runChoices(project('imported',null,'h2g'))).toEqual({primary:'start',secondary:[]});
  expect(runChoices(project('interrupted','t','h2g'))).toEqual({primary:'continue',secondary:[]});
 });
 it('shows the plugin\'s own stages as its progress.sh reports them',()=>{
  expect(skillStages({})).toEqual([]);
  const running=fixture('progress-running');
  const stages=skillStages(running);
  expect(stages).toHaveLength(running.stages!.length);
  expect(stages[0]).toEqual({label:'what is already here',detail:'a fresh workspace',state:'done'});
  expect(stages[1]).toEqual({label:'prerequisites',detail:'',state:'done'});
  expect(stages.find(s=>s.label==='prepare the input (prerender or static build)')).toEqual({label:'prepare the input (prerender or static build)',detail:'12 routes',state:'done'});
  expect(stages.find(s=>s.label==='commerce specimen (shops)')?.state).toBe('skipped');
  expect(stages.find(s=>s.label==='gates A + A2, reported')).toEqual({label:'gates A + A2, reported',detail:'A: 2 of 9 pages over threshold (report only)',state:'warned'});
  expect(stages.filter(s=>s.state==='current').map(s=>s.label)).toEqual(['the service builds the theme']);
  expect(stages.at(-1)?.state).toBe('waiting');
  expect(skillSummary(running)).toBe('Flash conversion · 59% · next: the theme screenshot');
  expect(skillSummary(fixture('progress-finished'))).toBe('Flash conversion · finished');
  const stopped=fixture('progress-stopped');
  expect(skillSummary(stopped)).toBe('Flash conversion · stopped at prepare the input (prerender or static build)');
  expect(skillStages(stopped).find(s=>s.state==='failed')?.detail).toBe('the build needs a DATABASE_URL the project does not provide');
  expect(skillSummary({})).toBeNull();
  // A malformed entry from the file is left out, never rendered.
  expect(skillStages({stages:[{stage:'x'} as never,{stage:'y',label:'Ok'}]}).map(s=>s.label)).toEqual(['Ok']);
 });
});

describe('after delivery: changes in the live preview, a rebuild only on purpose',()=>{
 it('a turn on a delivered HTML theme is a change',()=>{
  expect(changeTurn(project('deliverable_ready','t','html'))).toBe(true);
  expect(changeTurn(project('running','t','html'))).toBe(false);
  expect(changeTurn(project('deliverable_ready','t','astro'))).toBe(true);
  expect(changeTurn(project('deliverable_ready','t','h2g'))).toBe(true);
  expect(runChoices(project('deliverable_ready','t','h2g'))).toBeNull();
  expect(pendingRelease({count:3,sinceZip:2,changedSinceZip:true})).toBe('2 changes since the last release.');
  expect(pendingRelease({count:null,sinceZip:null,changedSinceZip:true})).toBe('The theme changed since the last release.');
  expect(pendingRelease({count:null,sinceZip:null,changedSinceZip:false})).toBeNull();
 });
 it('Start over warns what it replaces, in the owner\'s words',()=>{
  expect(startOverNote({count:3,sinceZip:0,changedSinceZip:false})).toBe('This converts the site again from its original source. All changes made after delivery (3) are lost and the current theme is replaced. Use this only if something is broken.');
  expect(startOverNote({count:3,sinceZip:1,changedSinceZip:true})).toContain('The last change is in no ZIP yet: use Make release first to keep it in a ZIP.');
  expect(startOverNote({count:5,sinceZip:2,changedSinceZip:true})).toContain('The last 2 changes are in no ZIP yet');
 });
});

describe('a stopped run: repaired and continued, never started over by the AI',()=>{
 it('offers Repair and continue; a new run is the menu\'s Start over',()=>{
  expect(runChoices(project('failed','t'),true)).toEqual({primary:'continue',secondary:[]});
  expect(runChoices(project('failed','t'),false)).toEqual({primary:'continue',secondary:['flash','full']});
  expect(rebuildModes(project('failed','t'),true)).toEqual(['flash','full']);
  expect(startOverNote({count:0,sinceZip:0,changedSinceZip:false},true)).toContain('Continue repairs the stopped stage instead.');
 });
 it('shows the plugin\'s repairs as they happen, then as the run ended',()=>{
  const live=[{stage:'3.5',attempt:1,of:2,lever:'article-part-residue',label:'the article layout from the site\'s own article',outcome:'open' as const,by:'run' as const}];
  expect(repairRows({repairs:live},{})).toEqual([{stage:'3.5',attempt:'1/2',lever:'the article layout from the site\'s own article',outcome:'Repairing…',by:'In the run'}]);
  expect(repairRows({},{repairs:[{stage:'5.6',attempt:2,of:2,lever:'cart-sync',outcome:'failed',by:'owner'}]})).toEqual([{stage:'5.6',attempt:'2/2',lever:'cart-sync',outcome:'Not fixed',by:'Your repair'}]);
  expect(repairRows({},{repairs:null})).toEqual([]);
 });
});
