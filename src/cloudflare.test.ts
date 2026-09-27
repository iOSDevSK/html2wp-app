import {describe,it,expect} from 'vitest';
import {deployGate,domainError,domainSummary,needsReplaceConsent,pendingDomain,projectNameError,stageState,t,type DomainState} from './cloudflare';
import type {Project} from './types';

const at=(phase:string,lastStep:string|null=null)=>({phase,lastStep,gates:[],preview:null} as unknown as Project);
const domain=(status:string,zoneInAccount:boolean):DomainState=>({name:'www.example.com',status,zoneInAccount,apex:false,record:{type:'CNAME',name:'www',content:'site.pages.dev'},dashboardUrl:'https://dash.cloudflare.com/a/pages/view/site/domains',url:'https://www.example.com'});

describe('Cloudflare Pages deploy',()=>{
 it('is enabled once the conversion has started and nothing else is running',()=>{
  expect(deployGate(at('imported'),false)).toEqual({enabled:false,reason:expect.stringContaining('Start the conversion')});
  // After that the native side reports whether the built site is there.
  expect(deployGate(at('running'),false).enabled).toBe(true);
  expect(deployGate(at('deliverable_ready'),false).enabled).toBe(true);
  expect(deployGate(at('deliverable_ready'),true)).toEqual({enabled:false,reason:expect.stringContaining('finishes')});
 });
 it('validates the project name and the optional domain like the native side',()=>{
  for(const ok of ['a','my-site','site-2',`${'a'.repeat(58)}`])expect(projectNameError(ok)).toBeNull();
  for(const bad of ['','-a','a-','My','a_b','a b',`${'a'.repeat(59)}`])expect(projectNameError(bad)).not.toBeNull();
  for(const ok of ['','www.example.com','Shop.Example.SK.','kaviareň.sk'])expect(domainError(ok)).toBeNull();
  for(const bad of ['https://example.com','example.com/a','localhost','a..com','-a.com','user@x.com'])expect(domainError(bad)).not.toBeNull();
 });
 it('offers replacing an existing project only for that specific refusal',()=>{
  expect(needsReplaceConsent('A Pages project named "x" already exists in this Cloudflare account. Choose another project name, or confirm that this site should replace it.')).toBe(true);
  expect(needsReplaceConsent('Cloudflare could not be reached.')).toBe(false);
 });
 it('shows progress per stage and skips the domain when none is set',()=>{
  expect(stageState('project','upload',false)).toBe('done');
  expect(stageState('upload','upload',false)).toBe('current');
  expect(stageState('domain','upload',true)).toBe('waiting');
  expect(stageState('domain','upload',false)).toBe('skipped');
  expect(stageState('domain','done',true)).toBe('done');
 });
 it('explains the domain state for the owner, with the record while pending',()=>{
  expect(domainSummary(domain('active',true))).toMatchObject({tone:'success',title:'www.example.com is connected'});
  expect(domainSummary(domain('pending',true)).text).toContain('Activate domain');
  expect(domainSummary(domain('pending',false)).text).toContain('registrar');
  expect(domainSummary(domain('error',false)).tone).toBe('warning');
  expect(pendingDomain(domain('pending',false))).toBe(true);
  expect(pendingDomain(domain('active',false))).toBe(false);
  expect(pendingDomain(null)).toBe(false);
 });
 it('keeps every label in one table and derives the pages.dev hint from the name',()=>{
  expect(t.projectHint('my-site')).toContain('https://my-site.pages.dev');
  expect(t.deploy).toBe('Deploy Astro 5 to Cloudflare Pages');
 });
});
