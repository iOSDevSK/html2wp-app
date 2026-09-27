import {describe,it,expect} from 'vitest';
import {isLocalPreviewUrl,sitePreviewBlocker} from './SitePreview';
import type {Project} from './types';

const at=(phase:string,lastStep:string|null=null)=>({phase,lastStep,gates:[],preview:null} as unknown as Project);
describe('built site preview',()=>{
 it('is available once the static build exists, for any theme type',()=>{
  expect(sitePreviewBlocker(at('imported'),false,null)).toContain('Start the conversion');
  // Once the conversion runs, the native side says whether the built site is there.
  expect(sitePreviewBlocker(at('running'),false,{distReady:false,distReason:'No build yet',url:null})).toBe('No build yet');
  expect(sitePreviewBlocker({...at('deliverable_ready'),target:'gutenberg'} as Project,false,{distReady:true,distReason:null,url:null})).toBeNull();
  expect(sitePreviewBlocker(at('deliverable_ready'),true,null)).toContain('finishes');
  expect(sitePreviewBlocker(at('failed','build'),false,{distReady:false,distReason:'No build yet',url:null})).toBe('No build yet');
 });
 it('opens and copies only the loopback preview address',()=>{
  expect(isLocalPreviewUrl('http://127.0.0.1:51234/')).toBe(true);
  for(const bad of ['http://localhost:51234/','https://127.0.0.1:1/','http://127.0.0.1:1/x','http://evil.example/','http://127.0.0.1.evil:80/'])expect(isLocalPreviewUrl(bad)).toBe(false);
 });
});
