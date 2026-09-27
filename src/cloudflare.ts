import type {Project} from './types';

/** Every string of the Cloudflare Pages panel, in one place for translation. */
export const t={
 rowTitle:'Astro 5 on Cloudflare Pages',
 rowIntro:'Publish the built Astro 5 site on Cloudflare Pages: a free https address on pages.dev, or your own domain.',
 deploy:'Deploy Astro 5 to Cloudflare Pages',redeploy:'Redeploy Astro 5',deployingRow:'Deploying…',liveBadge:'Live',
 remove:'Remove from Cloudflare Pages',removeTitle:'Remove this Pages project?',removeConfirm:'Yes, remove the Pages project',removing:'Removing…',
 eyebrow:'PUBLISH',title:'Publish your Astro 5 site.',
 intro:'We upload the static Astro 5 site built during the conversion. Your WordPress theme is not affected.',
 accountStep:'Cloudflare account',projectStep:'Site address',deployStep:'Deploy',
 signIn:'Sign in to Cloudflare',signingIn:'Opening Cloudflare…',waiting:'Approve access in the browser window that opened. This panel updates when you finish.',
 reopen:'Open sign-in page again',cancel:'Cancel',signOut:'Sign out',signedInAs:'Signed in as',
 signInNote:'You sign in on Cloudflare’s own page. The app receives a sign-in limited to Pages, your account list and domain lookups; it stays on this computer and is never shared with the AI assistant or included in exports.',
 chooseAccount:'Account',chooseAccountPlaceholder:'Choose an account',
 projectName:'Project name',projectHint:(name:string)=>`Your site will be available at https://${name||'…'}.pages.dev (Cloudflare adds a suffix if the name is taken).`,
 domain:'Custom domain (optional)',domainHint:'For example www.example.com. Leave empty to use the pages.dev address.',
 replace:(name:string)=>`Replace the existing Cloudflare project “${name}” with this site`,
 deploying:'Deploying…',checking:'Checking your Cloudflare sign-in…',
 live:'Your site is live.',open:'Open',copy:'Copy',check:'Check',checkingDomain:'Checking…',openDashboard:'Open in Cloudflare',
 lastDeploy:'Last deployed',notDeployed:'Not deployed yet',
 recordIntro:'Add this DNS record where your domain is managed:',
};

export type CloudflareAccount={id:string;name:string};
export type Identity={loggedIn:boolean;email:string|null;accounts:CloudflareAccount[]};
export type DomainState={name:string;status:string;detail?:string|null;zoneInAccount:boolean;apex:boolean;record:{type:string;name:string;content:string};dashboardUrl:string;url:string};
export type CloudflareSite={projectName:string;cfProjectId:string|null;domain:string|null;accountId:string|null;pagesUrl:string|null;deploymentUrl:string|null;deployedAt:string|null;domainState:DomainState|null};
export type CloudflareStatus={distReady:boolean;distReason:string|null;site:CloudflareSite;identity:Identity;accountId:string|null;authError:string|null;signedInLocally:boolean};
export type CloudflareProgress={projectId:string;stage:'project'|'upload'|'domain'|'done';message:string};

/** Mirrors the native rule: 1–58 lowercase letters, digits and hyphens. */
export function projectNameError(name:string):string|null{
 return /^[a-z0-9](?:[a-z0-9-]{0,56}[a-z0-9])?$/.test(name)?null:'Use 1–58 lowercase letters, digits or hyphens, not starting or ending with a hyphen.';
}
/** Light check before the native IDNA validation; empty is allowed. */
export function domainError(domain:string):string|null{
 const d=domain.trim().replace(/\.$/,'').toLowerCase();
 if(!d)return null;
 if(/[/:@?#\s]/.test(d)||!d.includes('.'))return 'Enter a domain like www.example.com, without https:// or a path.';
 return d.split('.').every(l=>l.length>0&&l.length<=63&&!l.startsWith('-')&&!l.endsWith('-'))?null:'This is not a valid domain name.';
}
/** Before asking the native side, which checks that dist/index.html exists: the static build runs in the Building stage. */
export function deployGate(project:Pick<Project,'phase'|'lastStep'|'gates'|'preview'>,running:boolean):{enabled:boolean;reason:string|null}{
 if(running)return {enabled:false,reason:'Available when the running conversion step finishes.'};
 if(project.phase==='imported')return {enabled:false,reason:'Start the conversion first. The site can be deployed once its static build is ready.'};
 return {enabled:true,reason:null};
}
/** The native side asks for consent before reusing a project it did not create. */
export function needsReplaceConsent(message:string):boolean{return message.includes('already exists in this Cloudflare account')}
export const stages:{key:CloudflareProgress['stage'];label:string}[]=[{key:'project',label:'Prepare the Pages project'},{key:'upload',label:'Upload the site'},{key:'domain',label:'Connect the domain'}];
export function stageState(key:CloudflareProgress['stage'],current:CloudflareProgress['stage']|null,withDomain:boolean):'done'|'current'|'waiting'|'skipped'{
 if(key==='domain'&&!withDomain)return 'skipped';
 const order=['project','upload','domain','done'];
 if(!current)return 'waiting';
 const i=order.indexOf(key),c=order.indexOf(current);
 return i<c?'done':i===c?'current':'waiting';
}
/** What the owner should know about the custom domain right now. */
export function domainSummary(d:DomainState):{tone:'success'|'warning'|'';title:string;text:string}{
 if(d.status==='active')return {tone:'success',title:`${d.name} is connected`,text:'Visitors reach your site on this domain with https.'};
 if(['blocked','error','deactivated'].includes(d.status))return {tone:'warning',title:`${d.name} could not be connected`,text:d.detail||'Cloudflare reported a problem with this domain. Open it in Cloudflare for details.'};
 if(d.zoneInAccount)return {tone:'',title:`${d.name} is being connected`,text:'The domain’s DNS is in this Cloudflare account. Cloudflare normally connects it within a few minutes. If it stays pending, open it in Cloudflare and choose Activate domain, or add the record below in the DNS settings.'};
 return {tone:'',title:`${d.name} is waiting for DNS`,text:'Add the record below at your domain registrar or DNS provider, then choose Check. DNS changes can take from a few minutes up to a day. HTTPS is issued automatically afterwards.'};
}
export const pendingDomain=(d:DomainState|null|undefined)=>!!d&&!['active','blocked','error','deactivated'].includes(d.status);
