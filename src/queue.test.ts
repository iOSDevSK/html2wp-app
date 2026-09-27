import {describe,it,expect} from 'vitest';
import {MESSAGE_LIMIT,queuedLabel,takeQueued} from './queue';

describe('changes queued while a change runs',()=>{
 it('go out as one message, every text verbatim and numbered',()=>{
  const queue=['Make the hero title larger.','Use the brand green on buttons.\nKeep the outline style.','Fix the footer link.'];
  const {text,rest}=takeQueued(queue);
  expect(rest).toEqual([]);
  expect(text).toBe('The owner sent these changes while you were working. Apply all of them in one pass, then rebuild once:\n1. Make the hero title larger.\n2. Use the brand green on buttons.\nKeep the outline style.\n3. Fix the footer link.');
 });
 it('send a single change as it was typed',()=>{
  expect(takeQueued(['Only this.'])).toEqual({text:'Only this.',rest:[]});
  expect(takeQueued([])).toEqual({text:'',rest:[]});
 });
 it('keep what does not fit the message limit for the next turn',()=>{
  const long='x'.repeat(20000);
  const {text,rest}=takeQueued([long,long,'short']);
  expect(text).toBe(long);
  expect(rest).toEqual([long,'short']);
  const again=takeQueued(rest);
  expect(again.text.length).toBeLessThanOrEqual(MESSAGE_LIMIT);
  expect(again.text).toContain('1. '+long);
  expect(again.text).toContain('2. short');
  expect(again.rest).toEqual([]);
 });
 it('say that the owner can keep typing',()=>{
  expect(queuedLabel(1)).toBe('1 change queued: it is sent when the current change finishes.');
  expect(queuedLabel(3)).toBe('3 changes queued: they are applied together when the current change finishes.');
 });
});
