import {describe,expect,it} from 'vitest';
import {shot2ai} from './previewBrowser';

describe('preview browser description',()=>{
 it('uses the default browser before and after opening',()=>{
  expect(shot2ai(null)).toBe('Opens in your default browser.');
  expect(shot2ai({browser:'default',extension:null})).toBe('Opens in your default browser.');
 });
 it('does not advertise an extension left over from the managed browser',()=>{
  expect(shot2ai({browser:'154.0',extension:{version:'0.4.0',sha:'3dab645'}})).toBe('Opens in your default browser.');
 });
});
