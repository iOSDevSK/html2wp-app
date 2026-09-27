import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
export const native=isTauri();
export async function call<T>(command:string,args?:Record<string,unknown>):Promise<T>{if(!native)throw new Error('Open the desktop application to use this feature. This browser view is a UI preview.');return invoke<T>(command,args);}
export async function on<T>(event:string,handler:(value:T)=>void):Promise<UnlistenFn>{if(!native)return()=>{};return listen<T>(event,event=>handler(event.payload));}
export const phaseLabel:Record<string,string>={imported:'Ready to convert',running:'Converting',deliverable_ready:'Ready to download',interrupted:'Paused',failed:'Needs attention',preparing:'Paused',converting_remote:'Paused',verifying:'Paused',review_required:'Paused',packaging:'Paused',preview_updated:'Paused',needs_decision:'Paused'};
export function errorText(e:unknown):string{return e instanceof Error?e.message:String(e);}
export function time(value:string):string{return new Date(value).toLocaleString('en',{month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'});}
