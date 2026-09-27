import {describe,it,expect} from 'vitest';
import {phaseLabel,errorText} from './bridge';
import type {ThreadStartParams} from '../schemas/codex/v2/ThreadStartParams';
import type {DynamicToolSpec} from '../schemas/codex/v2/DynamicToolSpec';
import tools from '../runtime/tools.json';
describe('Codex protocol contract',()=>{
 it('uses the generated dynamic tool function shape',()=>{
  const specs:DynamicToolSpec[]=tools.map(t=>({...t,type:'function' as const}));
  const params:ThreadStartParams={cwd:'/home/agent/empty',sandbox:'read-only',approvalPolicy:'on-request',dynamicTools:specs};
  expect(params.dynamicTools).toHaveLength(3);
  expect(tools.map(t=>t.name)).toEqual(["report_progress","sandbox_exec","project_shell"]);
  expect(specs.every(t=>t.type==='function'&&t.inputSchema)).toBe(true);
 });
 it('does not equate agent completion with delivery',()=>{
  expect(phaseLabel.running).toBe('Converting');
  expect(phaseLabel.deliverable_ready).toBe('Ready to download');
  expect(phaseLabel.failed).toBe('Needs attention');
  // An earlier release's phases read as a run to continue.
  expect(phaseLabel.review_required).toBe('Paused');
  expect(phaseLabel.needs_decision).toBe('Paused');
 });
 it('preserves actionable backend errors',()=>expect(errorText('Docker is not running')).toBe('Docker is not running'));
});
