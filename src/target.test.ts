import {describe,it,expect} from 'vitest';
import {fromTheme,switchableTargets,h2gStages,h2gStage,h2gElapsed,lastActivity,canChangeTarget,EXPERIMENTAL_NOTE,experimental,projectTarget,targetLabel,targetName,themeTargets} from './workflow';
import type {Project} from './types';

const imported={phase:'imported',gates:[],lastStep:null,threadId:null,name:'Site'} as unknown as Project;
describe('theme type',()=>{
 it('offers HTML first as the default and treats older projects as HTML',()=>{
  expect(themeTargets.map(t=>t.value)).toEqual(['html','gutenberg','astro','h2g']);
  expect(projectTarget(imported)).toBe('html');
  expect(targetLabel(imported)).toBe('HTML WordPress theme');
  expect(targetLabel({...imported,target:'gutenberg'})).toBe('Gutenberg block theme');
 });
 it('marks Gutenberg experimental wherever the owner sees it',()=>{
  expect(targetName('gutenberg')).toBe('Gutenberg block theme (experimental)');
  expect(targetName('gutenberg',true)).toBe('Gutenberg (experimental)');
  expect([targetName('html'),targetName('html',true),targetName('astro',true)]).toEqual(['HTML WordPress theme','HTML','Astro 5']);
  expect(themeTargets.filter(t=>experimental(t.value)).map(t=>t.value)).toEqual(['gutenberg']);
  expect(EXPERIMENTAL_NOTE).toBe('Experimental in this version: full Gutenberg support comes next.');
 });
 it('offers the Astro 5 project as its own output',()=>{
  const a={...imported,target:'astro'} as Project;
  expect(projectTarget(a)).toBe('astro');
  expect(targetLabel(a)).toBe('Astro 5 project only');
 });
 it('can change only before the conversion starts',()=>{
  expect(canChangeTarget(imported)).toBe(true);
  expect(canChangeTarget({...imported,lastStep:'analyze'})).toBe(false);
  expect(canChangeTarget({...imported,threadId:'thread'})).toBe(false);
  expect(canChangeTarget({...imported,phase:'preparing'})).toBe(false);
 });
 it('offers Gutenberg from an HTML theme for a theme html2wp made, never switched with a site\'s types',()=>{
  const h2g=themeTargets.find(t=>t.value==='h2g')!;
  expect(h2g.label).toBe('Gutenberg from an HTML theme');
  expect(h2g.detail).toBe('Turns an HTML WordPress theme made by html2wp into a native block theme, editable in the block editor. Usually takes 3–5 hours.');
  expect(fromTheme('h2g')).toBe(true);
  expect(switchableTargets('h2g')).toEqual(['h2g']);
  expect(switchableTargets('html')).toEqual(['html','astro']);
  expect(projectTarget({target:'h2g'})).toBe('h2g');
 });
 it('shows the skill\'s nine steps, where the run is and how long it has run',()=>{
  expect(h2gStages.map(([l])=>l)).toEqual(['Audit','Scaffold and theme.json','Fonts','CSS','JS','Parts, templates and patterns','Content','Importer and setup','Verify']);
  expect(h2gStage({phase:'imported'},{})).toBe(-1);
  expect(h2gStage({phase:'preparing'},{current:4})).toBe(3);
  expect(h2gStage({phase:'deliverable_ready'},{current:9})).toBe(9);
  const started=Date.parse('2026-09-24T10:00:00Z');
  expect(h2gElapsed({},started)).toBeNull();
  expect(h2gElapsed({started:'2026-09-24T10:00:00Z'},started+42*60000)).toBe('42 min so far · usually 3–5 hours');
  expect(h2gElapsed({started:'2026-09-24T10:00:00Z'},started+135*60000)).toBe('2 h 15 min so far · usually 3–5 hours');
  expect(lastActivity(undefined,started)).toBeNull();
  expect(lastActivity('2026-09-24T10:00:00Z',started+20000)).toBe('last activity just now');
  expect(lastActivity('2026-09-24T10:00:00Z',started+7*60000)).toBe('last activity 7 min ago');
 });
});
