/** The longest message the host takes (send_message). */
export const MESSAGE_LIMIT=32000;
const LEAD='The owner sent these changes while you were working. Apply all of them in one pass, then rebuild once:';

/** The owner's messages queued while a change ran, as ONE message for the
 * next turn: every text verbatim and numbered, so one build, conversion and
 * check covers them all. What does not fit the host's limit stays queued. */
export function takeQueued(queue:string[]):{text:string;rest:string[]}{
 if(queue.length<2)return {text:queue[0]||'',rest:[]};
 const items:string[]=[];
 for(const message of queue){
  const next=[...items,`${items.length+1}. ${message}`];
  if(items.length&&[LEAD,...next].join('\n').length>MESSAGE_LIMIT)break;
  items.push(next[next.length-1]);
 }
 // A single oversized message goes alone, as it would have before.
 if(items.length===1)return {text:queue[0],rest:queue.slice(1)};
 return {text:[LEAD,...items].join('\n'),rest:queue.slice(items.length)};
}

/** What the chat says about the queue, so the owner knows to keep typing. */
export function queuedLabel(count:number):string{
 return count===1?'1 change queued: it is sent when the current change finishes.':`${count} changes queued: they are applied together when the current change finishes.`;
}
