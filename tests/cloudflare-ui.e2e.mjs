// Built-site preview and Cloudflare Pages panel on the production bundle with a
// mocked native side: sign-in, account choice, replace consent, progress,
// pending → active domain, local preview start/stop and its disabled state.
// Nothing reaches Cloudflare; the real deploy is the owner's manual check.
import {test,expect} from '@playwright/test';
import {readFileSync,readdirSync} from 'node:fs';
const ACCOUNT_A='a'.repeat(32),ACCOUNT_B='b'.repeat(32);
async function boot(page,{distReady=true}={}){
 const assets='dist/assets';
 const read=suffix=>readFileSync(`${assets}/${readdirSync(assets).find(file=>file.endsWith(suffix))}`,'utf8');
  await page.setViewportSize({width:1380,height:900});
 await page.setContent('<html><head></head><body><div id="root"></div></body></html>');
 await page.evaluate(({A,B,distReady})=>{
  const f=window.cf={calls:[],callbacks:{},events:{},next:1,signedIn:false,distReady,site:null,preview:null};
  const now=new Date().toISOString();
  f.project={id:'fixture',name:'Kaviareň Web',sourceName:'site.zip',kind:'Static HTML',createdAt:now,updatedAt:now,phase:'deliverable_ready',revision:1,threadId:'t',pages:[],gates:[],artifacts:[],preview:null,runtimeImage:'img',pluginCommit:'abc',reporting:'not_required',lastError:null};
  f.emit=(event,payload)=>f.callbacks[f.events[event]]?.({event,payload});
  const identity=()=>f.signedIn?{loggedIn:true,email:'owner@example.invalid',accounts:[{id:A,name:'Agency'},{id:B,name:'Client'}]}:{loggedIn:false,email:null,accounts:[]};
  const saved=()=>f.site||{projectName:'kaviaren-web',cfProjectId:null,domain:null,accountId:null,pagesUrl:null,deploymentUrl:null,deployedAt:null,domainState:null};
  window.isTauri=true;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
  window.__TAURI_INTERNALS__={
   transformCallback:fn=>{const id=f.next++;f.callbacks[id]=fn;return id},
   unregisterCallback:id=>delete f.callbacks[id],
   invoke:async(command,args={})=>{
    f.calls.push({command,args});
    if(command==='plugin:event|listen'){f.events[args.event]=args.handler;return args.handler}
    if(command==='plugin:event|unlisten'||command==='save_queue'||command==='set_active_project')return;
    if(command==='get_bootstrap')return {projects:[f.project],versions:{},platform:'macos',architecture:'aarch64',disclosureAccepted:true,licenceConfigured:false,lastUpdateCheck:null,activeProject:null};
    if(command==='check_runtime')return {ready:true,docker:true,imageReady:true,message:'ready'};
    if(command==='account_read')return {account:{type:'chatgpt',email:'fixture@example.invalid'}};
    if(command==='project_detail')return {messages:[],activity:[],queue:[]};
    if(command==='pending_question')return null;
    if(command==='visual_edit_lite')return {available:false};
    if(command==='cloudflare_site')return {distReady:f.distReady,distReason:f.distReady?null:'Available once the conversion has built the static site (its “HTML to Astro” stage).',site:saved()};
    if(command==='cloudflare_status')return {distReady:true,distReason:null,site:saved(),identity:identity(),accountId:null,authError:null,signedInLocally:f.signedIn};
    if(command==='cloudflare_login_start')return {url:'https://dash.cloudflare.com/oauth2/auth?redirect_uri=http%3A%2F%2Flocalhost%3A8976%2Foauth%2Fcallback'};
    if(command==='cloudflare_login_wait')return new Promise(resolve=>{f.finishLogin=()=>{f.signedIn=true;resolve(identity())}});
    if(command==='cloudflare_select_account')return;
    if(command==='cloudflare_deploy'){
     if(!args.replaceExisting)throw 'A Pages project named "kaviaren-web" already exists in this Cloudflare account. Choose another project name, or confirm that this site should replace it.';
     return new Promise(resolve=>{f.finishDeploy=()=>{f.site={projectName:args.projectName,domain:args.domain,accountId:args.accountId,pagesUrl:'https://kaviaren-web.pages.dev',deploymentUrl:'https://abc.kaviaren-web.pages.dev',deployedAt:new Date().toISOString(),domainState:{name:args.domain,status:'pending',zoneInAccount:false,apex:false,record:{type:'CNAME',name:args.domain,content:'kaviaren-web.pages.dev'},dashboardUrl:`https://dash.cloudflare.com/${args.accountId}/pages/view/kaviaren-web/domains`,url:`https://${args.domain}`}};resolve(f.site)}});
    }
    if(command==='cloudflare_domain_check'){f.site={...f.site,domainState:{...f.site.domainState,status:'active'}};return f.site}
    if(command==='cloudflare_open'||command==='open_url')return;
    if(command==='site_preview_status')return {distReady:f.distReady,distReason:f.distReady?null:'Available once the conversion has built the static site (its “HTML to Astro” stage).',url:f.preview};
    if(command==='site_preview_start'){f.preview='http://127.0.0.1:51234/';return {url:f.preview}}
    if(command==='site_preview_stop'){if(args.exceptProject!==f.project.id)f.preview=null;return}
    throw Error('Unexpected fixture command '+command);
   }};
 },{A:ACCOUNT_A,B:ACCOUNT_B,distReady});
 await page.addStyleTag({content:read('.css')});
 await page.addScriptTag({content:read('.js'),type:'module'});
}
test('deploy a converted site to Cloudflare Pages',async({page})=>{
 const assets='dist/assets';
 const read=suffix=>readFileSync(`${assets}/${readdirSync(assets).find(file=>file.endsWith(suffix))}`,'utf8');
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setViewportSize({width:1380,height:900});
 await page.setContent('<html><head></head><body><div id="root"></div></body></html>');
 await page.evaluate(({A,B})=>{
  const f=window.cf={calls:[],callbacks:{},events:{},next:1,signedIn:false,distReady:true,site:null};
  const now=new Date().toISOString();
  f.project={id:'fixture',name:'Kaviareň Web',sourceName:'site.zip',kind:'Static HTML',createdAt:now,updatedAt:now,phase:'deliverable_ready',revision:1,threadId:'t',pages:[],gates:[],artifacts:[],preview:null,runtimeImage:'img',pluginCommit:'abc',reporting:'not_required',lastError:null};
  f.emit=(event,payload)=>f.callbacks[f.events[event]]?.({event,payload});
  const identity=()=>f.signedIn?{loggedIn:true,email:'owner@example.invalid',accounts:[{id:A,name:'Agency'},{id:B,name:'Client'}]}:{loggedIn:false,email:null,accounts:[]};
  const saved=()=>f.site||{projectName:'kaviaren-web',cfProjectId:null,domain:null,accountId:null,pagesUrl:null,deploymentUrl:null,deployedAt:null,domainState:null};
  window.isTauri=true;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
  window.__TAURI_INTERNALS__={
   transformCallback:fn=>{const id=f.next++;f.callbacks[id]=fn;return id},
   unregisterCallback:id=>delete f.callbacks[id],
   invoke:async(command,args={})=>{
    f.calls.push({command,args});
    if(command==='plugin:event|listen'){f.events[args.event]=args.handler;return args.handler}
    if(command==='plugin:event|unlisten'||command==='save_queue'||command==='set_active_project')return;
    if(command==='get_bootstrap')return {projects:[f.project],versions:{},platform:'macos',architecture:'aarch64',disclosureAccepted:true,licenceConfigured:false,lastUpdateCheck:null,activeProject:null};
    if(command==='check_runtime')return {ready:true,docker:true,imageReady:true,message:'ready'};
    if(command==='account_read')return {account:{type:'chatgpt',email:'fixture@example.invalid'}};
    if(command==='project_detail')return {messages:[],activity:[],queue:[]};
    if(command==='pending_question')return null;
    if(command==='visual_edit_lite')return {available:false};
    if(command==='cloudflare_site')return {distReady:f.distReady,distReason:f.distReady?null:'Available once the conversion has built the static site (its “HTML to Astro” stage).',site:saved()};
    if(command==='cloudflare_status')return {distReady:true,distReason:null,site:saved(),identity:identity(),accountId:null,authError:null,signedInLocally:f.signedIn};
    if(command==='cloudflare_login_start')return {url:'https://dash.cloudflare.com/oauth2/auth?redirect_uri=http%3A%2F%2Flocalhost%3A8976%2Foauth%2Fcallback'};
    if(command==='cloudflare_login_wait')return new Promise(resolve=>{f.finishLogin=()=>{f.signedIn=true;resolve(identity())}});
    if(command==='cloudflare_select_account')return;
    if(command==='cloudflare_deploy'){
     if(!args.replaceExisting)throw 'A Pages project named "kaviaren-web" already exists in this Cloudflare account. Choose another project name, or confirm that this site should replace it.';
     return new Promise(resolve=>{f.finishDeploy=()=>{f.site={projectName:args.projectName,cfProjectId:'cf-kaviaren-web',domain:args.domain,accountId:args.accountId,pagesUrl:'https://kaviaren-web.pages.dev',deploymentUrl:'https://abc.kaviaren-web.pages.dev',deployedAt:new Date().toISOString(),domainState:{name:args.domain,status:'pending',zoneInAccount:false,apex:false,record:{type:'CNAME',name:args.domain,content:'kaviaren-web.pages.dev'},dashboardUrl:`https://dash.cloudflare.com/${args.accountId}/pages/view/kaviaren-web/domains`,url:`https://${args.domain}`}};resolve(f.site)}});
    }
    if(command==='cloudflare_remove'){
     if(f.denyRemove)throw 'Cloudflare refused to remove the Pages project.';
     f.site={projectName:'kaviaren-web',cfProjectId:null,domain:null,accountId:null,pagesUrl:null,deploymentUrl:null,deployedAt:null,domainState:null};return f.site;
    }
    if(command==='cloudflare_domain_check'){f.site={...f.site,domainState:{...f.site.domainState,status:'active'}};return f.site}
    if(command==='cloudflare_open')return;
    throw Error('Unexpected fixture command '+command);
   }};
 },{A:ACCOUNT_A,B:ACCOUNT_B});
 await page.addStyleTag({content:read('.css')});
 await page.addScriptTag({content:read('.js'),type:'module'});
 await page.getByRole('tab',{name:/Exports/}).click();
 const open=page.getByRole('button',{name:'Deploy Astro 5 to Cloudflare Pages'});
 await expect(open).toBeEnabled();
 // Nothing touches Docker until the panel opens.
 expect(await page.evaluate(()=>window.cf.calls.some(c=>c.command==='cloudflare_status'))).toBe(false);
 await open.click();
 const panel=page.getByRole('dialog',{name:'Publish your Astro 5 site.'});
 await panel.getByRole('button',{name:'Sign in to Cloudflare'}).click();
 await expect(panel.getByText('Approve access in the browser window')).toBeVisible();
 await page.evaluate(()=>window.cf.finishLogin());
 await expect(panel.getByText('owner@example.invalid')).toBeVisible();
 const deploy=panel.getByRole('button',{name:'Deploy Astro 5 to Cloudflare Pages'});
 await expect(panel.getByLabel('Project name')).toHaveValue('kaviaren-web');
 await expect(deploy).toBeDisabled(); // two accounts, none chosen yet
 await panel.getByLabel('Account').selectOption(ACCOUNT_B);
 await panel.getByLabel('Project name').fill('Bad_Name');
 await expect(panel.getByText('Use 1–58 lowercase letters')).toBeVisible();
 await expect(deploy).toBeDisabled();
 await panel.getByLabel('Project name').fill('kaviaren-web');
 await panel.getByLabel('Custom domain (optional)').fill('https://www.example.com');
 await expect(deploy).toBeDisabled();
 await panel.getByLabel('Custom domain (optional)').fill('www.kaviaren.sk');
 await deploy.click();
 await expect(panel.getByRole('alert').getByText('already exists in this Cloudflare account')).toBeVisible();
 await expect(deploy).toBeDisabled();
 await panel.getByLabel(/Replace the existing Cloudflare project/).check();
 await deploy.click();
 await page.evaluate(()=>window.cf.emit('cloudflare-progress',{projectId:'fixture',stage:'upload',message:'Uploading'}));
 await expect(panel.locator('.cf-stages li.done')).toHaveText('Prepare the Pages project');
 await expect(panel.locator('.cf-stages li.current')).toHaveText('Upload the site');
 await page.evaluate(()=>window.cf.finishDeploy());
 await expect(panel.getByText('Your site is live.')).toBeVisible();
 const call=await page.evaluate(()=>window.cf.calls.filter(c=>c.command==='cloudflare_deploy').at(-1).args);
 expect(call).toEqual({projectId:'fixture',projectName:'kaviaren-web',domain:'www.kaviaren.sk',accountId:ACCOUNT_B,replaceExisting:true});
 await expect(panel.getByText('www.kaviaren.sk is waiting for DNS')).toBeVisible();
 await expect(panel.getByRole('table',{name:'DNS record'})).toContainText('kaviaren-web.pages.dev');
 await panel.getByRole('button',{name:'Open',exact:true}).first().click();
 expect(await page.evaluate(()=>window.cf.calls.find(c=>c.command==='cloudflare_open').args)).toEqual({projectId:'fixture',target:'pages'});
 await panel.getByRole('button',{name:'Check',exact:true}).click();
 await expect(panel.getByText('www.kaviaren.sk is connected')).toBeVisible();
 await panel.getByRole('button',{name:'Close'}).click();
 await expect(page.getByRole('button',{name:'Redeploy'})).toBeVisible();
 await expect(page.getByText('www.kaviaren.sk',{exact:true})).toBeVisible();
 await page.locator('.cf-row-actions .cf-remove-link').click();
 await expect(page.getByRole('dialog').getByText(`Cloudflare account ${ACCOUNT_B}`)).toBeVisible();
 await page.getByRole('button',{name:'Cancel'}).click();
 expect(await page.evaluate(()=>window.cf.calls.some(c=>c.command==='cloudflare_remove'))).toBe(false);
 await panel.locator('.cf-remove-link').click();
 await page.evaluate(()=>{window.cf.denyRemove=true});
 await page.getByRole('button',{name:'Yes, remove the Pages project'}).click();
 await expect(page.getByRole('alert').getByText('Cloudflare refused to remove')).toBeVisible();
 await expect(page.getByRole('dialog').getByText('Your site is live.')).toBeVisible();
 await page.evaluate(()=>{window.cf.denyRemove=false});
 await page.getByRole('button',{name:'Yes, remove the Pages project'}).click();
 await expect(page.getByText('Not deployed yet')).toBeVisible();
 await expect(page.getByRole('button',{name:'Remove from Cloudflare Pages'})).toHaveCount(0);
 expect(errors).toEqual([]);
});

test('preview the built site on this computer',async({page})=>{
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await boot(page);
 await page.getByRole('tab',{name:/Exports/}).click();
 const row=page.locator('.site-preview-row');
 await row.getByRole('button',{name:'Preview Astro 5 site'}).click();
 await expect(row.getByText('Running')).toBeVisible();
 await expect(row.locator('code')).toHaveText('http://127.0.0.1:51234/');
 expect(await page.evaluate(()=>window.cf.calls.filter(c=>c.command==='open_url').map(c=>c.args.url))).toEqual(['http://127.0.0.1:51234/']);
 await row.getByRole('button',{name:'Open'}).click();
 await expect.poll(()=>page.evaluate(()=>window.cf.calls.filter(c=>c.command==='open_url').length)).toBe(2);
 await row.getByRole('button',{name:'Stop preview'}).click();
 await expect(row.getByRole('button',{name:'Preview Astro 5 site'})).toBeEnabled();
 // Leaving the project ends its preview.
 const stops=await page.evaluate(()=>window.cf.calls.filter(c=>c.command==='site_preview_stop').map(c=>c.args));
 expect(stops).toContainEqual({exceptProject:'fixture'});
 await page.getByRole('button',{name:'All projects'}).click();
 await expect.poll(()=>page.evaluate(()=>window.cf.calls.filter(c=>c.command==='site_preview_stop').at(-1).args)).toEqual({exceptProject:null});
 expect(errors).toEqual([]);
});

test('both actions explain why they wait for the static build',async({page})=>{
 await boot(page,{distReady:false});
 await page.getByRole('tab',{name:/Exports/}).click();
 await expect(page.getByRole('button',{name:'Preview Astro 5 site'})).toBeDisabled();
 await expect(page.getByRole('button',{name:'Deploy Astro 5 to Cloudflare Pages'})).toBeDisabled();
 await expect(page.getByText('Available once the conversion has built the static site (its “HTML to Astro” stage).')).toHaveCount(2);
});
