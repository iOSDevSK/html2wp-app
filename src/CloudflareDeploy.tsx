import {useCallback,useEffect,useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import {ArrowSquareOut,ArrowsClockwise,Check,CheckCircle,CircleNotch,Cloud,CloudArrowUp,Copy,SignOut,Trash,WarningCircle,X} from '@phosphor-icons/react';
import {call,errorText,on,time} from './bridge';
import {t,deployGate,domainError,domainSummary,needsReplaceConsent,pendingDomain,projectNameError,stages,stageState,type CloudflareProgress,type CloudflareSite,type CloudflareStatus,type Identity} from './cloudflare';
import type {Project} from './types';
import './cloudflare.css';

type Notify=(message:string,error?:boolean)=>void;
type Saved={distReady:boolean;distReason:string|null;site:CloudflareSite};

/** Exports row: the built site's Cloudflare Pages address and the deploy panel. */
export function CloudflareRow({project,running,notify}:{project:Project;running:boolean;notify:Notify}){
 const[saved,setSaved]=useState<Saved|null>(null);const[open,setOpen]=useState(false);const[deploying,setDeploying]=useState(false);const[removeIntent,setRemoveIntent]=useState(false);
 const load=useCallback(()=>{let alive=true;call<Saved>('cloudflare_site',{projectId:project.id}).then(v=>{if(alive)setSaved(v)}).catch(()=>{if(alive)setSaved(null)});return()=>{alive=false}},[project.id]);
 useEffect(()=>load(),[load,project.updatedAt,open]);
 const gate=deployGate(project,running);const ready=gate.enabled&&(saved?saved.distReady:true);const reason=gate.reason||(saved&&!saved.distReady?saved.distReason:null);
 const site=saved?.site;const live=site?.domainState?.status==='active'?site.domainState.url:site?.pagesUrl;
 return <div className="export-row">
  <span className="export-icon"><Cloud size={32} weight="duotone"/></span>
  <div><h3>{t.rowTitle} {site?.deployedAt&&!deploying&&<span className="status success"><span className="status-dot"/>{t.liveBadge}</span>}</h3><p>{live&&site?.deployedAt?<>{live.replace('https://','')}</>:t.rowIntro}</p><small>{reason||(site?.deployedAt?`${t.lastDeploy} ${time(site.deployedAt)}`:t.notDeployed)}</small></div>
  <div className="cf-row-actions"><button className={`button ${site?.deployedAt?'':'primary'}`} type="button" disabled={!ready||deploying} title={reason||undefined} onClick={()=>{setRemoveIntent(false);setOpen(true)}}>{deploying?<CircleNotch className="spin"/>:<CloudArrowUp/>}{deploying?t.deployingRow:site?.deployedAt?t.redeploy:t.deploy}</button>
   {site?.pagesUrl&&<button className="button cf-remove-link" type="button" disabled={running||deploying} onClick={()=>{setRemoveIntent(true);setOpen(true)}}><Trash/>{t.remove}</button>}</div>
  {open&&createPortal(<CloudflarePanel project={project} notify={notify} initialRemove={removeIntent} onDeploying={setDeploying} onClose={()=>setOpen(false)}/>,document.body)}
 </div>;
}

export function CloudflarePanel({project,notify,onClose,onDeploying,initialRemove=false}:{project:Project;notify:Notify;onClose:()=>void;onDeploying?:(v:boolean)=>void;initialRemove?:boolean}){
 const[status,setStatus]=useState<CloudflareStatus|null>(null);const[loadError,setLoadError]=useState('');
 const[login,setLogin]=useState<'idle'|'starting'|'waiting'>('idle');const[loginUrl,setLoginUrl]=useState('');
 const[account,setAccount]=useState('');const[name,setName]=useState('');const[domain,setDomain]=useState('');
 const[deploying,setDeploying]=useState(false);const[stage,setStage]=useState<CloudflareProgress['stage']|null>(null);
 const[error,setError]=useState('');const[replace,setReplace]=useState(false);const[offerReplace,setOfferReplace]=useState(false);
 const[site,setSite]=useState<CloudflareSite|null>(null);const[checking,setChecking]=useState(false);const[working,setWorking]=useState(false);
 const[removing,setRemoving]=useState(false);const[confirmRemove,setConfirmRemove]=useState(initialRemove);
 const alive=useRef(true);const polls=useRef(0);const panelRef=useRef<HTMLDivElement>(null);
 const refresh=useCallback(async()=>{
  setLoadError('');
  try{const s=await call<CloudflareStatus>('cloudflare_status',{projectId:project.id});if(!alive.current)return;setStatus(s);setAccount(a=>a||s.accountId||'');setName(n=>n||s.site.projectName);setDomain(d=>d||s.site.domain||'');if(s.site.deployedAt)setSite(s.site)}
  catch(e){if(alive.current)setLoadError(errorText(e))}
 },[project.id]);
 useEffect(()=>{alive.current=true;void refresh();return()=>{alive.current=false}},[refresh]);
 useEffect(()=>{const un=on<CloudflareProgress>('cloudflare-progress',p=>{if(p.projectId===project.id)setStage(p.stage)});return()=>{void un.then(f=>f())}},[project.id]);
 useEffect(()=>{const key=(e:KeyboardEvent)=>{if(e.key==='Escape'&&!deploying&&!removing)onClose()};document.addEventListener('keydown',key);(document.querySelector('.cf-panel button') as HTMLElement|null)?.focus();return()=>document.removeEventListener('keydown',key)},[deploying,removing,onClose]);
 useEffect(()=>onDeploying?.(deploying),[deploying,onDeploying]);
 const identity:Identity|undefined=status?.identity;const signedIn=!!identity?.loggedIn;
 async function signIn(){
  setError('');setLogin('starting');
  try{const r=await call<{url:string}>('cloudflare_login_start');if(!alive.current)return;setLoginUrl(r.url);setLogin('waiting');
   await call<Identity>('cloudflare_login_wait');if(!alive.current)return;setLogin('idle');await refresh()}
  catch(e){if(alive.current){setLogin('idle');setError(errorText(e))}}
 }
 async function cancelLogin(){await call('cloudflare_login_cancel').catch(()=>{});setLogin('idle')}
 async function signOut(){setWorking(true);setError('');try{await call('cloudflare_logout');setAccount('');await refresh();notify('Signed out of Cloudflare. The saved sign-in was removed from this computer.')}catch(e){setError(errorText(e))}finally{setWorking(false)}}
 async function pickAccount(id:string){setAccount(id);if(!id)return;try{await call('cloudflare_select_account',{accountId:id})}catch(e){setError(errorText(e))}}
 async function deploy(){
  setError('');setDeploying(true);setStage('project');
  try{const s=await call<CloudflareSite>('cloudflare_deploy',{projectId:project.id,projectName:name,domain,accountId:account,replaceExisting:replace});if(!alive.current)return;setSite(s);setStage('done');setOfferReplace(false);setReplace(false);polls.current=0;panelRef.current?.scrollTo({top:0,behavior:'smooth'})}
  catch(e){const m=errorText(e);if(alive.current){setError(m);setOfferReplace(needsReplaceConsent(m));setStage(null)}}
  finally{if(alive.current)setDeploying(false)}
 }
 async function removeSite(){
  setError('');setRemoving(true);
  try{const cleared=await call<CloudflareSite>('cloudflare_remove',{projectId:project.id});if(!alive.current)return;setSite(null);setStatus(s=>s?{...s,site:cleared}:s);setConfirmRemove(false);notify('The Cloudflare Pages project was removed. Your local site and exports are still here.');onClose()}
  catch(e){if(alive.current)setError(errorText(e))}
  finally{if(alive.current)setRemoving(false)}
 }
 const checkDomain=useCallback(async(quiet=false)=>{
  setChecking(true);
  try{const s=await call<CloudflareSite>('cloudflare_domain_check',{projectId:project.id});if(alive.current)setSite(s)}
  catch(e){if(alive.current&&!quiet)setError(errorText(e))}
  finally{if(alive.current)setChecking(false)}
 },[project.id]);
 // While a domain is pending, look again every 30 seconds for up to 20 minutes.
 useEffect(()=>{if(!pendingDomain(site?.domainState)||deploying)return;const timer=setInterval(()=>{if(polls.current++<40)void checkDomain(true)},30000);return()=>clearInterval(timer)},[site?.domainState?.status,deploying,checkDomain]);
 async function copy(text:string){try{await navigator.clipboard.writeText(text);notify('Copied to clipboard.')}catch{notify('Clipboard is unavailable. Select and copy the text.',true)}}
 const openSite=(target:'pages'|'deployment'|'domain'|'dashboard')=>void call('cloudflare_open',{projectId:project.id,target}).catch(e=>notify(errorText(e),true));
 const nameError=name?projectNameError(name):null;const hostError=domainError(domain);
 const canDeploy=signedIn&&!!account&&!!name&&!nameError&&!hostError&&!deploying&&!removing&&login==='idle'&&status?.distReady!==false&&(!offerReplace||replace);
 const d=site?.domainState;const summary=d?domainSummary(d):null;
 return <div className="modal-backdrop" onClick={()=>{if(!deploying&&!removing)onClose()}}>
  <div ref={panelRef} className="modal cf-panel" role="dialog" aria-modal="true" aria-labelledby="cf-title" onClick={e=>e.stopPropagation()}>
   <button className="modal-close icon-button" onClick={onClose} disabled={deploying||removing} aria-label="Close"><X/></button>
   <span className="eyebrow">{t.eyebrow} · {t.rowTitle.toUpperCase()}</span><h2 id="cf-title">{t.title}</h2><p>{t.intro}</p>
   {site?.pagesUrl&&site.deployedAt&&<section className="cf-result" aria-live="polite">
    <p className="cf-live"><CheckCircle size={18}/>{t.live} <small>{t.lastDeploy} {time(site.deployedAt)}</small></p>
    <div className="cf-url"><code>{site.pagesUrl}</code><button className="button" type="button" onClick={()=>openSite('pages')}><ArrowSquareOut/>{t.open}</button></div>
    {d&&summary&&<div className={`cf-domain ${summary.tone}`}>
     <div className="cf-url"><code>{d.url}</code>{d.status==='active'&&<button className="button" type="button" onClick={()=>openSite('domain')}><ArrowSquareOut/>{t.open}</button>}</div>
     <strong>{summary.title}</strong><p>{summary.text}</p>
     {d.status!=='active'&&<><p className="cf-hint">{t.recordIntro}</p><div className="cf-record" role="table" aria-label="DNS record">
      <div role="row"><span role="columnheader">Type</span><span role="columnheader">Name</span><span role="columnheader">Target</span></div>
      <div role="row"><code role="cell">{d.record.type}</code><code role="cell">{d.record.name}<button type="button" aria-label="Copy name" onClick={()=>void copy(d.record.name)}><Copy/></button></code><code role="cell">{d.record.content}<button type="button" aria-label="Copy target" onClick={()=>void copy(d.record.content)}><Copy/></button></code></div>
     </div>
     <div className="button-row"><button className="button" type="button" disabled={checking} onClick={()=>void checkDomain()}>{checking?<CircleNotch className="spin"/>:<ArrowsClockwise/>}{checking?t.checkingDomain:t.check}</button><button className="button text" type="button" onClick={()=>openSite('dashboard')}>{t.openDashboard}<ArrowSquareOut/></button></div></>}
    </div>}
   </section>}
   {status?.distReady===false&&<div className="cf-alert" role="status"><WarningCircle size={18}/><span>{status.distReason}</span></div>}
   <section className="cf-step"><h3><span>1</span>{t.accountStep}</h3>
    {!status&&!loadError?<p className="cf-muted"><CircleNotch className="spin"/> {t.checking}</p>:signedIn?<>
     <div className="cf-account"><div className="avatar">{(identity?.email?.[0]||'C').toUpperCase()}</div><div><small>{t.signedInAs}</small><strong>{identity?.email||'Cloudflare account'}</strong></div><button className="button text" type="button" disabled={working||deploying} onClick={()=>void signOut()}><SignOut/>{t.signOut}</button></div>
     {(identity?.accounts.length||0)>1&&<label className="cf-field">{t.chooseAccount}<select value={account} disabled={deploying} onChange={e=>void pickAccount(e.target.value)}><option value="">{t.chooseAccountPlaceholder}</option>{identity!.accounts.map(a=>
      <option key={a.id} value={a.id}>{a.name||a.id}</option>)}</select></label>}
     {identity?.accounts.length===0&&<p className="cf-error">This sign-in has no Cloudflare account with access to Pages.</p>}
    </>:<>
     {login==='waiting'?<div className="cf-waiting" role="status"><CircleNotch className="spin"/><p>{t.waiting}</p><div className="button-row"><button className="button" type="button" onClick={()=>void call('cloudflare_open_login',{url:loginUrl}).catch(e=>setError(errorText(e)))}><ArrowSquareOut/>{t.reopen}</button><button className="button text" type="button" onClick={()=>void cancelLogin()}>{t.cancel}</button></div></div>
      :<button className="button primary" type="button" disabled={login!=='idle'||!!loadError&&!status} onClick={()=>void signIn()}>{login==='starting'?<CircleNotch className="spin"/>:<Cloud/>}{login==='starting'?t.signingIn:t.signIn}</button>}
     <p className="cf-note">{t.signInNote}</p>
    </>}
    {(loadError||status?.authError)&&<p className="cf-error" role="alert">{loadError||status?.authError}</p>}
   </section>
   <section className="cf-step"><h3><span>2</span>{t.projectStep}</h3>
    <label className="cf-field">{t.projectName}<input value={name} disabled={deploying} spellCheck={false} autoComplete="off" onChange={e=>{setName(e.target.value.toLowerCase());setOfferReplace(false);setReplace(false)}} aria-invalid={!!nameError}/></label>
    <small className={nameError?'cf-error':'cf-hint'}>{nameError||t.projectHint(name)}</small>
    <label className="cf-field">{t.domain}<input value={domain} disabled={deploying} spellCheck={false} autoComplete="off" placeholder="www.example.com" onChange={e=>setDomain(e.target.value)} aria-invalid={!!hostError}/></label>
    <small className={hostError?'cf-error':'cf-hint'}>{hostError||t.domainHint}</small>
   </section>
   <section className="cf-step"><h3><span>3</span>{t.deployStep}</h3>
    {(deploying||stage)&&<ol className="cf-stages">{stages.map(s=>{const st=stageState(s.key,stage,!!domain.trim());return st==='skipped'?null:<li key={s.key} className={st}>{st==='done'?<Check/>:st==='current'&&deploying?<CircleNotch className="spin"/>:<span className="cf-dot"/>}{s.label}</li>})}</ol>}
    {error&&<div className="cf-alert" role="alert"><WarningCircle size={18}/><span>{error}</span></div>}
    {offerReplace&&<label className="cf-check"><input type="checkbox" checked={replace} onChange={e=>setReplace(e.target.checked)}/>{t.replace(name)}</label>}
    <button className="button primary" type="button" disabled={!canDeploy} onClick={()=>void deploy()}>{deploying?<CircleNotch className="spin"/>:<CloudArrowUp/>}{deploying?t.deploying:site?.deployedAt?t.redeploy:t.deploy}</button>
   </section>
   {site?.pagesUrl&&<section className="cf-step cf-remove"><h3><Trash size={17}/>{t.remove}</h3>
    {confirmRemove?<div className="cf-remove-confirm" role="group" aria-label={t.removeTitle}>
     <strong>{t.removeTitle}</strong><p>Cloudflare account <code>{site.accountId}</code> · project <code>{site.projectName}</code>. This deletes its deployments and takes its Pages address and connected domains offline. Your local project and exports stay here; external DNS records are not removed.</p>
     <div className="button-row"><button className="button cf-danger" type="button" disabled={removing||deploying||!signedIn} onClick={()=>void removeSite()}>{removing?<CircleNotch className="spin"/>:<Trash/>}{removing?t.removing:t.removeConfirm}</button><button className="button text" type="button" disabled={removing} onClick={()=>setConfirmRemove(false)}>{t.cancel}</button></div>
    </div>:<button className="button cf-remove-link" type="button" disabled={removing||deploying||!signedIn} onClick={()=>setConfirmRemove(true)}><Trash/>{t.remove}</button>}
    {!signedIn&&<small className="cf-hint">Sign in to the saved Cloudflare account before removing this Pages project.</small>}
   </section>}
  </div>
 </div>;
}
