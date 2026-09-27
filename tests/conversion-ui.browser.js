// Production UI with native IPC fixtures: the Flash and Full buttons, the
// progress view fed from the skill's progress file, Stop, Continue and the
// delivered files in Exports. No service, login or conversion.
export default async function checkConversionUi(page,bundle){
 const view=await page.context().newPage();const pageErrors=[];view.on('pageerror',e=>pageErrors.push(e.stack||e.message));
 try{
  await view.setViewportSize({width:1380,height:900});
  await view.setContent('<html><head></head><body><div id="root"></div></body></html>');
  await view.evaluate(progress=>{
   const now=new Date().toISOString();
   const f=window.conversionFixture={calls:[],callbacks:{},events:{},next:1,progress:{},h2g:{}};
   f.project={id:'fixture',name:'Lovable · UI regression',sourceName:'fixture.zip',kind:'Web app',createdAt:now,updatedAt:now,phase:'imported',revision:1,threadId:null,pages:[],gates:[],artifacts:[],preview:null,runtimeImage:'fixture',pluginCommit:'fixture-commit',reporting:'not_required',lastError:null,target:'html'};
   f.theme={id:'theme-project',name:'Clara theme',sourceName:'clara.zip',kind:'html2wp HTML theme',createdAt:now,updatedAt:now,phase:'imported',revision:1,threadId:null,pages:[],gates:[],artifacts:[],preview:null,runtimeImage:'fixture',pluginCommit:'fixture-commit',reporting:'not_required',lastError:null,target:'h2g'};
   f.fixtures=progress;
   f.emit=(event,payload)=>f.callbacks[f.events[event]]({event,payload});
   const find=id=>id==='theme-project'?f.theme:f.project;
   window.isTauri=true;
   window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
   window.__TAURI_INTERNALS__={
    transformCallback:fn=>{const id=f.next++;f.callbacks[id]=fn;return id},
    unregisterCallback:id=>delete f.callbacks[id],
    invoke:async(command,args={})=>{
     f.calls.push({command,args});
     if(command==='plugin:event|listen'){f.events[args.event]=args.handler;return args.handler}
     if(command==='plugin:event|unlisten'||command==='set_active_project'||command==='site_preview_stop')return;
     if(command==='save_queue'){f.savedQueue=args.messages;return}
     if(command==='set_max_parallel'){f.maxParallel=args.value;return args.value}
     if(command==='set_experimental_gutenberg'){f.experimentalGutenberg=args.enabled;return args.enabled}
     if(command==='get_bootstrap')return {experimentalGutenberg:!!f.experimentalGutenberg,projects:[f.project,f.theme],versions:{appVersion:'1.0.0',pluginVersion:'1.0.0-gamma.1',pluginCommit:'fixture-commit',codexVersion:'0.154.0',adapterVersion:'1.0.0'},platform:'macos',architecture:'aarch64',disclosureAccepted:true,licenceConfigured:false,lastUpdateCheck:null,activeProject:null};
     if(command==='check_runtime')return {ready:true,docker:true,imageReady:true,message:'Fixture ready'};
     if(command==='account_read')return {account:{type:'chatgpt',email:'fixture@example.invalid'}};
     if(command==='project_detail')return {project:structuredClone(find(args.projectId)),messages:[{id:'m1',projectId:args.projectId,role:'assistant',text:'Your project is ready. Choose Flash for a fast conversion, or Full for the complete, checked one.',createdAt:now}],activity:[],queue:[]};
     if(command==='pending_question')return null;
     if(command==='skill_progress')return structuredClone({progress:f.progress,result:f.result||{},changes:f.changes||null,stoppedRun:!!f.stoppedRun});
     if(command==='send_message'){f.sent=[...(f.sent||[]),args.text];return {}}
     // Make release: the host's package_theme; a changed theme is the next revision (the host adds it and says project-updated).
     if(command==='package_theme'){f.packaged=(f.packaged||0)+1;if(f.packaged===1){f.changes={count:2,sinceZip:0,changedSinceZip:false};f.project.revision=2;f.project.artifacts=[...f.project.artifacts,{id:'a-r2',revision:2,filename:'clara-hayes-1.0.0-r2.zip',sha256:'y',createdAt:new Date().toISOString(),kind:'theme',reviewed:true,checks:'packaged'}];f.project.updatedAt=new Date().toISOString();f.emit('project-updated',structuredClone(f.project));return {changed:true,revision:2,filename:'clara-hayes-1.0.0-r2.zip'}}return {changed:false,revision:2,filename:'clara-hayes-1.0.0-r2.zip'}}
     if(command==='set_project_target'){f.project.target=args.target;f.project.updatedAt=new Date().toISOString();return structuredClone(f.project)}
     if(command==='h2g_progress')return structuredClone(f.h2g);
     if(command==='start_run'){f.run=args;const p=find(args.projectId);p.phase='running';p.threadId='thread-'+args.projectId;if(args.mode==='flash'||args.mode==='full')p.flash=args.mode==='flash';p.updatedAt=new Date().toISOString();return {turn:{id:'turn-1'}}}
     // As the host's mark_stopped: a delivered project stays delivered.
     if(command==='stop_conversion'){if(f.project.phase!=='deliverable_ready')f.project.phase='interrupted';f.project.updatedAt=new Date().toISOString();return}
     if(command==='model_catalog')return {models:[{model:'fixture-default',displayName:'Fixture default',isDefault:true,supportedReasoningEfforts:[{reasoningEffort:'medium'}],defaultReasoningEffort:'medium'}],selectedModel:'fixture-default',selectedEffort:''};
     if(command==='visual_edit_lite')return {available:true,version:'9.9.9'};
     if(command==='site_preview_status')return f.staticReady?{distReady:true,distReason:null,url:null}:{distReady:false,distReason:'This conversion did not build a static site.',url:null};
     if(command==='cloudflare_status')return {signedIn:false,accounts:[]};
     if(command==='cloudflare_site')return f.staticReady?{distReady:true,distReason:null,site:{projectName:'',domain:null,accountId:null,pagesUrl:null,deploymentUrl:null,deployedAt:null,domainState:null}}:null;
     if(command==='download_project'){
      f.downloads=[...(f.downloads||[]),args];
      if(f.holdDownload){f.downloadPending=true;return new Promise(resolve=>{f.finishDownload=()=>{f.downloadPending=false;f.holdDownload=false;resolve({exactArchive:false,files:3})}})}
      return {exactArchive:false,files:3};
     }
     if(command==='plugin:dialog|save')return '/fixture/exports/'+(args.options?.defaultPath||'file');
     if(command==='export_artifact'){f.saved=args;return}
     if(command==='delete_previous_artifact'){const p=find(args.projectId);p.artifacts=p.artifacts.filter(a=>a.id!==args.artifactId);p.updatedAt=new Date().toISOString();f.emit('project-updated',structuredClone(p));return structuredClone(p)}
     if(command==='plugin:dialog|message'){const answer=f.confirmNext??'Ok';delete f.confirmNext;return answer}
     if(command==='preview_status')return structuredClone(f.site||{available:false});
     if(command==='preview_action'){f.site={...f.site,running:args.action==='start'};return structuredClone(f.site)}
     if(command==='open_url'){f.opened=args.url;return}
     if(command==='preview_browser_status')return {browser:'default',extension:null,note:null};
     if(command==='preview_open'){f.previewOpened=[...(f.previewOpened||[]),args.page];return {browser:'default',extension:null,note:null}}
     if(command==='compare_index')return structuredClone(f.review||{});
     if(command==='compare_status')return structuredClone(f.compareStatus||{state:null,note:null,startedAt:null,updatedAt:null,running:false,error:null});
     if(command==='compare_generate'){f.compareStatus={state:'running',note:'capturing desktop (1440px)',startedAt:now,updatedAt:now,running:true,error:null};return {started:true}}
     if(command==='compare_image'){f.shownImages=[...(f.shownImages||[]),args.path];return f.fixtures.review.images[args.path]||(args.path.startsWith('compare/')?f.fixtures.review.images['visual-review/front-page.side-by-side.png']:undefined)}
     throw new Error('Unexpected command in fixture: '+command);
    }
   };
  },bundle.progress);
  await view.addStyleTag({content:bundle.css});
  await view.addScriptTag({content:bundle.js,type:'module'});
  const heading=view.locator('.project-heading');
  await heading.getByRole('heading',{name:'Lovable · UI regression'}).waitFor();

  // The experimental card is off by default and can be toggled without hiding old projects.
  await view.getByRole('button',{name:/New project/}).click();
  if(await view.getByRole('dialog').getByRole('radio',{name:/Gutenberg/}).count())throw new Error('Gutenberg must be hidden by default');
  await view.getByRole('button',{name:'Close',exact:true}).click();
  await view.getByRole('button',{name:/^Settings/}).click();
  await view.getByRole('combobox',{name:'Conversions at once'}).selectOption('6');
  await view.waitForFunction(()=>window.conversionFixture.maxParallel===6);
  await view.getByRole('switch',{name:'Gutenberg conversion'}).check();
  await view.waitForFunction(()=>window.conversionFixture.experimentalGutenberg===true);
  await view.getByRole('button',{name:/New project/}).click();
  await view.getByRole('dialog').getByRole('radio',{name:/Gutenberg from an HTML theme/}).check();
  await view.getByRole('button',{name:'Close',exact:true}).click();
  await view.getByRole('switch',{name:'Gutenberg conversion'}).uncheck();
  await view.waitForFunction(()=>window.conversionFixture.experimentalGutenberg===false);
  await view.getByRole('button',{name:/New project/}).click();
  if(await view.getByRole('dialog').getByRole('radio',{name:/Gutenberg/}).count())throw new Error('Off must hide the card again');
  if(!await view.getByRole('dialog').getByRole('radio',{name:/HTML WordPress theme/}).isChecked())throw new Error('Off must reset a pending Gutenberg import');
  await view.getByRole('button',{name:'Close',exact:true}).click();
  await view.locator('.project-nav').getByRole('button',{name:/Lovable/}).click();

  await view.getByLabel('Manage project').click();
  const menu=view.locator('.project-menu>div');
  const positions=await menu.locator('button svg').evaluateAll(nodes=>nodes.map(n=>n.getBoundingClientRect().x));
  if(Math.max(...positions)-Math.min(...positions)>1)throw new Error('Project menu icons must align');
  if(await menu.getByRole('button',{name:'Download original ZIP'}).count())throw new Error('Original download belongs in Exports, not the menu');
  await view.getByLabel('Manage project').click();
  await view.getByRole('tab',{name:'Exports'}).click();
  await view.getByRole('button',{name:'Download original ZIP'}).click();
  await view.waitForFunction(()=>window.conversionFixture.downloads?.some(x=>x.kind==='original'));
  await view.getByLabel('Manage project').click();
  await menu.getByRole('button',{name:'Download diagnostic logs'}).click();
  await view.waitForFunction(()=>window.conversionFixture.downloads?.some(x=>x.kind==='diagnostics'));
  await view.getByRole('button',{name:'Docs'}).click();
  await view.waitForFunction(()=>window.conversionFixture.opened==='https://html2wp.dev/docs/');
  await view.evaluate(()=>{window.conversionFixture.opened=null});
  await view.getByRole('tab',{name:'Overview'}).click();

  // A new project: Flash is the default, Full beside it; no manual review, no Review tab.
  const flash=heading.getByRole('button',{name:'Flash',exact:true}),full=heading.getByRole('button',{name:'Full',exact:true});
  await flash.waitFor();await full.waitFor();
  if(!(await flash.getAttribute('class')).includes('primary'))throw new Error('Flash must be the default (primary) conversion');
  if(await view.getByRole('tab',{name:/Review/}).count())throw new Error('The Review tab belongs to the removed host logic');
  if(await view.getByText('Review pages manually').count())throw new Error('Manual review mode was removed');
  await view.getByText('Flash: The fast conversion: the AI runs every stage once, no repair loops; red checks go into the report.',{exact:false}).waitFor();

  // The Astro 5 project is its own run; there is no Gutenberg target (the h2g card makes Gutenberg).
  if(await view.getByRole('radio',{name:/^Gutenberg/}).count())throw new Error('No Gutenberg target in 1.0.0');
  await view.getByRole('radio',{name:/Astro 5/}).check();
  await view.waitForFunction(()=>window.conversionFixture.project.target==='astro');
  const astro=heading.getByRole('button',{name:'Build Astro project'});
  await astro.waitFor();
  if(!(await astro.getAttribute('class')).includes('primary'))throw new Error('The Astro run leads for an Astro project');
  if(await heading.getByRole('button',{name:'Flash',exact:true}).count()||await heading.getByRole('button',{name:'Full',exact:true}).count())throw new Error('Flash and Full are the HTML theme\'s runs');
  await view.getByRole('radio',{name:/HTML/}).check();
  await view.waitForFunction(()=>window.conversionFixture.project.target==='html');
  await flash.waitFor();
  if(await view.getByText('BETA').count()||!(await view.locator('.version-pill').textContent()).startsWith('v1.0.0'))throw new Error('The version pill reads v1.0.0');
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-new.png')});

  // Flash starts the skill's run as a goal: the host sends start_run, nothing else.
  await flash.click();
  await view.waitForFunction(()=>window.conversionFixture.run?.mode==='flash');
  const run=await view.evaluate(()=>window.conversionFixture.run);
  if(run.projectId!=='fixture')throw new Error('Flash started another project: '+JSON.stringify(run));
  await view.evaluate(()=>{const f=window.conversionFixture;f.emit('turn-started',{projectId:'fixture'});f.emit('project-updated',structuredClone(f.project))});
  await heading.getByRole('button',{name:'Stop conversion'}).waitFor();
  await view.getByText('The plugin’s stages appear here as soon as it reports its first one.').waitFor();

  // The plugin's progress.sh writes progress.json: the Overview shows its stages, polled while it runs.
  await view.evaluate(()=>{const f=window.conversionFixture;f.progress=f.fixtures.running});
  const stages=view.locator('.pipeline.skill-stages li');
  const total=bundle.progress.running.stages.length;
  await view.waitForFunction(n=>document.querySelectorAll('.pipeline.skill-stages li').length===n,total,{timeout:12000});
  const states=await stages.evaluateAll(items=>items.map(li=>[li.querySelector('strong').textContent,li.className]));
  const cls={pending:'',running:'current',done:'done',warned:'warned',skipped:'skipped',failed:'failed'};
  const expected=bundle.progress.running.stages.map(s=>[s.label,cls[s.state]]);
  if(JSON.stringify(states)!==JSON.stringify(expected))throw new Error('Progress stages differ: '+JSON.stringify(states));
  await view.locator('.pipeline li.current',{hasText:'the service builds the theme'}).getByText('In progress').waitFor();
  await view.locator('.pipeline li.warned',{hasText:'gates A + A2, reported'}).getByText('A: 2 of 9 pages over threshold (report only)').waitFor();
  await view.locator('.pipeline li.skipped',{hasText:'commerce specimen'}).getByText('Skipped').waitFor();
  await view.locator('.h2g-elapsed',{hasText:'Flash conversion · 59% · next: the theme screenshot'}).waitFor();
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-running.png')});

  // The preview WordPress the plugin started: its address and login, started and stopped here.
  await view.getByRole('tab',{name:'Preview'}).click();
  await view.getByText('Not started yet').waitFor();
  if(await view.getByRole('button',{name:'Start preview'}).count())throw new Error('No preview to start before the plugin made one');
  await view.evaluate(()=>{const f=window.conversionFixture;f.site={available:true,url:'http://localhost:53412',user:'admin',password:'admin123',running:true,project:'h2wp-clara-hayes-3fa9c1'};f.project.updatedAt=new Date().toISOString();f.emit('project-updated',structuredClone(f.project))});
  await view.locator('.browser-address',{hasText:'http://localhost:53412'}).waitFor();
  const password=view.locator('.credential-field').filter({hasText:'Password'});
  await password.locator('.credential-actions').getByRole('button',{name:'Show password'}).scrollIntoViewIfNeeded();
  const eye=await password.locator('.credential-actions').getByRole('button',{name:'Show password'}).locator('svg').boundingBox();
  await view.mouse.click(eye.x+eye.width/2,eye.y+eye.height/2);
  await password.locator('code',{hasText:'admin123'}).waitFor();
  await password.locator('.credential-actions').getByRole('button',{name:'Hide password'}).click();
  if(await password.locator('code').textContent()==='admin123')throw new Error('Clicking again must hide the password');
  await password.getByRole('button',{name:'Show password'}).first().click();
  await password.locator('code',{hasText:'admin123'}).waitFor();
  await password.getByRole('button',{name:'Hide password'}).first().click();
  const actions=await password.locator('.credential-actions button').evaluateAll(buttons=>buttons.map(b=>b.getBoundingClientRect().x));
  if(actions[1]-actions[0]>45)throw new Error('The password eye must sit beside Copy');

  // Both preview buttons use the default browser without managed extension setup.
  await view.locator('.shot2ai-version',{hasText:'Opens in your default browser.'}).waitFor();
  await view.locator('.preview-cover').getByRole('button',{name:/Open website/}).click();
  await view.locator('.preview-cover').getByRole('button',{name:'WordPress admin'}).click();
  if(await view.evaluate(()=>JSON.stringify(window.conversionFixture.previewOpened))!=='["site","admin"]')throw new Error('The preview buttons open site and admin in the default browser');
  if(await view.evaluate(()=>window.conversionFixture.opened))throw new Error('Preview buttons bypassed preview_open');
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-preview.png')});
  await password.locator('.credential-actions').getByRole('button',{name:'Show password'}).click();
  await view.locator('.copy-field code',{hasText:'admin123'}).waitFor();
  await view.getByRole('button',{name:'Stop preview'}).click();
  await view.getByRole('button',{name:'Start preview'}).waitFor();
  await view.getByRole('button',{name:'Start preview'}).click();
  await view.locator('.preview-cover').getByRole('button',{name:/Open website/}).waitFor();
  await view.getByRole('tab',{name:'Overview'}).click();

  // Compare: on the owner's click the plugin captures the source beside WordPress; the owner pages through.
  await view.getByRole('tab',{name:'Compare'}).click();
  await view.getByText('No comparison yet').waitFor();
  const generate=view.getByRole('button',{name:'Generate comparison'});
  if(!(await generate.isDisabled()))throw new Error('Comparing waits while the conversion runs');
  await view.getByText('Available when the conversion finishes or is stopped.').first().waitFor();
  await view.getByRole('tab',{name:'Overview'}).click();

  // Stop, then Continue resumes the same run; Flash and Full start a new one.
  await heading.getByRole('button',{name:'Stop conversion'}).click();
  await view.waitForFunction(()=>window.conversionFixture.calls.some(c=>c.command==='stop_conversion'));
  await view.evaluate(()=>{const f=window.conversionFixture;f.emit('project-updated',structuredClone(f.project))});
  const resume=heading.getByRole('button',{name:'Continue'});
  await resume.waitFor();
  if(!(await resume.getAttribute('class')).includes('primary'))throw new Error('Continue must lead after a stop');
  await heading.getByRole('button',{name:'Flash',exact:true}).waitFor();await heading.getByRole('button',{name:'Full',exact:true}).waitFor();
  await resume.click();
  await view.waitForFunction(()=>window.conversionFixture.run?.mode==='continue');

  // The plugin stopped the run (v1.4): Repair and continue, the Repairs table, and no new run in the header.
  await view.evaluate(()=>{const f=window.conversionFixture;f.emit('turn-completed',{projectId:'fixture'});f.run=null;f.stoppedRun=true;f.project.phase='failed';f.project.updatedAt=new Date().toISOString();
   f.progress={...f.fixtures.running,state:'stopped',repairs:[{stage:'3.5',attempt:1,of:2,lever:'article-part-residue',label:'the article layout from the site\'s own article',outcome:'failed',by:'run'},{stage:'3.5',attempt:2,of:2,lever:'article-part-other',label:'another article',outcome:'failed',by:'run'}]};
   f.result={status:'stopped',stopped:{stage:'3.5',reason:'no article layout could be derived'},couldNotFix:[{stage:'3.5',signature:'article-part-foreign',what:'the article layout',levers:['the article layout from the site\'s own article','another article']}]};
   f.emit('project-updated',structuredClone(f.project))});
  const repair=heading.getByRole('button',{name:'Repair and continue'});
  await repair.waitFor();
  for(const name of ['Flash','Full'])if(await heading.getByRole('button',{name,exact:true}).count())throw new Error(`A stopped run offers no new ${name} run in the header`);
  await view.locator('.repairs tbody tr').nth(1).waitFor();
  if(JSON.stringify(await view.locator('.repairs tbody tr').first().locator('td').allTextContents())!==JSON.stringify(['3.5','1/2',"the article layout from the site's own articleIn the run",'Not fixed']))throw new Error('The Repairs table shows the plugin\'s rows');
  await view.locator('.could-not-fix li',{hasText:'Stage 3.5'}).getByText('Tried: the article layout from the site\'s own article, another article').waitFor();
  if(await view.getByRole('textbox',{name:'Message your assistant'}).getAttribute('placeholder')!=='Tell the AI what you know about the stop: it repairs that stage, then continues…')throw new Error('The composer says a message repairs the stop');
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-stopped.png')});
  await heading.locator('summary[aria-label="Manage project"]').click();
  await heading.getByRole('button',{name:'Start over from the original…'}).click();
  await view.getByRole('alertdialog',{name:'Start over from the original?'}).getByText('Continue repairs the stopped stage instead.',{exact:false}).waitFor();
  await view.getByRole('alertdialog').getByRole('button',{name:'Cancel'}).click();
  await repair.click();
  await view.waitForFunction(()=>window.conversionFixture.run?.mode==='continue');
  await view.evaluate(()=>{const f=window.conversionFixture;f.stoppedRun=false;f.result={};f.progress=f.fixtures.running;f.emit('project-updated',structuredClone(f.project))});

  // With the run over, the owner generates the comparison and walks through it.
  await view.evaluate(()=>window.conversionFixture.emit('turn-completed',{projectId:'fixture'}));
  await view.getByRole('tab',{name:'Compare'}).click();
  await view.getByRole('button',{name:'Generate comparison'}).click();
  // It runs in the background; the tab shows the plugin's note while it polls.
  await view.getByText('Comparing: capturing desktop (1440px)',{exact:false}).waitFor();
  if(!(await view.getByRole('button',{name:'Comparing…'}).isDisabled()))throw new Error('One comparison at a time');
  await view.evaluate(()=>{const f=window.conversionFixture;f.review=f.fixtures.review.index;f.compareStatus={state:'done',note:'4 side-by-side composite(s) of 3 page(s)',startedAt:null,updatedAt:null,running:false,error:null};f.emit('compare-finished',{projectId:'fixture',error:null})});
  await view.locator('.compare-meta h3',{hasText:'Home'}).waitFor();
  await view.locator('.compare-meta',{hasText:'0.42% different'}).locator('code',{hasText:'/'}).waitFor();
  await view.getByText('1 of 3').waitFor();
  await view.locator('.comparison img[alt*="Home"]').waitFor();
  await view.locator('.comparison-labels',{hasText:'Original site'}).getByText('WordPress conversion').waitFor();
  // Mobile: the same page at 390, with its own difference.
  await view.getByRole('button',{name:'Mobile · 390'}).click();
  await view.locator('.compare-meta',{hasText:'1.30% different'}).waitFor();
  await view.locator('.comparison-frame.mobile img[alt*="Home"]').waitFor();
  // Buttons fade between states (0.15 s): let them settle before a picture.
  if(bundle.screenshot){await view.waitForTimeout(400);await view.screenshot({path:bundle.screenshot.replace('.png','-compare-mobile.png')})}
  await view.getByRole('button',{name:'Desktop · 1440'}).click();
  await view.getByRole('button',{name:'Next page'}).click();
  await view.locator('.compare-meta',{hasText:'3.70% different'}).locator('code',{hasText:'/about/'}).waitFor();
  await view.locator('.comparison img[alt*="About"]').waitFor();
  await view.getByRole('button',{name:'Refresh selected page'}).click();
  await view.waitForFunction(()=>window.conversionFixture.calls.some(c=>c.command==='compare_generate'&&c.args.pageKey==='about'));
  await view.evaluate(()=>{const f=window.conversionFixture;f.compareStatus={state:'done',running:false};f.emit('compare-finished',{projectId:'fixture',error:null})});
  await view.locator('.compare-meta',{hasText:'About'}).waitFor();

  await view.getByRole('combobox',{name:'Page'}).selectOption('contact');
  await view.locator('.comparison',{hasText:'the page timed out after 60 s'}).waitFor();
  if(await view.getByRole('button',{name:'Next page'}).isEnabled())throw new Error('The last page has no next page');
  await view.getByRole('button',{name:'Previous page'}).click();
  await view.locator('.comparison img').click();
  await view.getByRole('dialog',{name:'Full page comparison'}).waitFor();
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-compare-zoom.png')});
  await view.getByRole('button',{name:'Close comparison'}).click();
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-compare.png')});
  const shown=await view.evaluate(()=>window.conversionFixture.shownImages);
  if(JSON.stringify([...new Set(shown)])!==JSON.stringify(['visual-review/front-page.side-by-side.png','visual-review/mobile/front-page.side-by-side.png','visual-review/about.side-by-side.png']))throw new Error('Images asked for: '+JSON.stringify(shown));
  if(await view.getByText('Looks good').count()||await view.getByText('Approve').count())throw new Error('The comparison approves nothing');
  await view.getByRole('tab',{name:'Overview'}).click();
  await view.evaluate(()=>window.conversionFixture.emit('turn-started',{projectId:'fixture'}));

  // The plugin's result says delivered: the host copied its files; Exports lists exactly them.
  await view.evaluate(()=>{const f=window.conversionFixture;f.progress=f.fixtures.finished;f.result={status:'delivered',verdict:f.fixtures.result.verdict};f.project.phase='deliverable_ready';f.project.updatedAt=new Date().toISOString();
   const at=new Date().toISOString(),file=(id,kind,filename)=>({id,revision:1,filename,sha256:'x',createdAt:at,kind,reviewed:true,checks:'flash'});
   f.project.artifacts=[file('a-report','report','CONVERSION-REPORT.md'),file('a-theme','theme','clara-hayes-1.0.0.zip'),file('a-astro','astro','clara-hayes-astro-1.0.0.zip'),file('a-pdf','pdf','conversion-report.pdf')];
   f.emit('turn-completed',{projectId:'fixture'});f.emit('project-updated',structuredClone(f.project))});
  await view.getByRole('tab',{name:'Exports',selected:true}).waitFor();
  await view.evaluate(()=>{const f=window.conversionFixture;f.staticReady=true;f.holdDownload=true;f.project.updatedAt=new Date().toISOString();f.emit('project-updated',structuredClone(f.project))});
  await view.getByRole('navigation',{name:'Projects'}).getByRole('button',{name:'Clara theme'}).click();
  await heading.getByRole('heading',{name:'Clara theme'}).waitFor();
  await heading.getByLabel('Manage project').click();
  await heading.getByRole('button',{name:'Download diagnostic logs'}).click();
  await view.waitForFunction(()=>window.conversionFixture.downloadPending===true);
  await view.evaluate(()=>window.conversionFixture.emit('turn-started',{projectId:'theme-project'}));
  await view.getByRole('navigation',{name:'Projects'}).getByRole('button',{name:/Lovable/}).click();
  await view.getByRole('tab',{name:'Exports'}).click();
  await view.getByRole('button',{name:'Preview Astro 5 site'}).waitFor();
  if(!await view.getByRole('button',{name:'Preview Astro 5 site'}).isEnabled())throw new Error('Another project must not block a built Astro preview');
  if(!await view.getByRole('button',{name:'Deploy Astro 5 to Cloudflare Pages'}).isEnabled())throw new Error('Another project must not block Cloudflare deployment');
  await view.evaluate(()=>{const f=window.conversionFixture;f.finishDownload();f.emit('turn-completed',{projectId:'theme-project'})});
  const rows=await view.locator('.export-rows .export-row h3').allTextContents();
  if(JSON.stringify(rows.slice(0,4))!==JSON.stringify(['WordPress theme','Conversion report (PDF)','Conversion report','Astro 5 project']))throw new Error('Exports rows: '+JSON.stringify(rows));
  await view.getByText('clara-hayes-1.0.0.zip').waitFor();
  await view.locator('.export-row',{hasText:'clara-hayes-1.0.0.zip'}).getByText('Flash conversion',{exact:false}).waitFor();
  await view.getByRole('tab',{name:'Overview'}).click();
  await view.locator('.skill-verdict',{hasText:'Flash: not visually repaired'}).waitFor();
  await view.locator('.h2g-elapsed',{hasText:'Flash conversion · finished'}).waitFor();
  await view.getByRole('tab',{name:'Exports'}).click();
  if(await view.getByText('Generate PDF').count()||await view.getByText('Build theme').count()||await view.getByText('Full check & build').count())throw new Error('Exports shows a removed host step');
  await view.locator('.export-row',{hasText:'clara-hayes-1.0.0.zip'}).getByRole('button',{name:'Save'}).click();
  await view.waitForFunction(()=>window.conversionFixture.saved?.artifactId==='a-theme');
  // After delivery a chat message is a change in the live preview: no "Converting", no new run.
  for(const name of ['Flash','Full','Rebuild','Continue'])if(await heading.getByRole('button',{name,exact:true}).count())throw new Error(`A delivered project shows no run button in its header (${name})`);
  await view.getByRole('tab',{name:'Overview'}).click();
  const composer=view.getByRole('textbox',{name:'Message your assistant'});
  if(await composer.getAttribute('placeholder')!=='Ask for a change: it is made in the live preview, no rebuild…')throw new Error('The composer says a change is made in the preview');
  await composer.fill('make the heading italic');
  await view.getByRole('button',{name:'Send message'}).click();
  await view.waitForFunction(()=>window.conversionFixture.sent?.[0]==='make the heading italic');
  await view.evaluate(()=>{const f=window.conversionFixture;f.emit('turn-started',{projectId:'fixture'})});
  await heading.getByText('Applying a change').waitFor();
  if(await heading.getByText('Converting').count())throw new Error('A change is not a conversion');
  await view.locator('.h2g-elapsed',{hasText:'Flash conversion · finished · applying your change in the preview'}).waitFor();
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-change.png')});
  await heading.getByRole('button',{name:'Stop change'}).click();
  await view.getByText('The change stopped. What was delivered and what the change already applied are kept.').waitFor();
  if(!await view.evaluate(()=>window.conversionFixture.calls.some(c=>c.command==='stop_conversion')))throw new Error('Stop reaches the host');
  // The change reached the preview: the chat and the Overview offer Make release; Exports has no packaging button.
  await view.evaluate(()=>{const f=window.conversionFixture;f.changes={count:2,sinceZip:2,changedSinceZip:true};f.project.updatedAt=new Date().toISOString();f.emit('project-updated',structuredClone(f.project))});
  await view.locator('.changes-line',{hasText:'Changes since last ZIP: 2'}).getByRole('button',{name:'Make release'}).waitFor();
  const cta=view.locator('.release-cta');
  await cta.getByText('2 changes since the last release.').waitFor();
  await cta.getByRole('button',{name:'Open Exports'}).waitFor();
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-release.png')});
  await view.getByRole('tab',{name:'Exports'}).click();
  await view.locator('.export-row',{hasText:'clara-hayes-1.0.0.zip'}).locator('.changed-note',{hasText:'Changed since this ZIP: 2 changes in the preview, not in a release yet'}).waitFor();
  if(await view.locator('.exports-page').getByRole('button',{name:/Get ZIP|Make release/}).count())throw new Error('Exports has no packaging button: a release is made from the chat or the Overview');
  // From the chat: packaged, and Exports opens on the new ZIP, marked, ready to Save.
  await view.getByRole('tab',{name:'Overview'}).click();
  await cta.getByRole('button',{name:'Make release'}).click();
  await view.getByRole('tab',{name:'Exports',selected:true}).waitFor();
  const release=view.locator('.export-row.released');
  await release.getByText('clara-hayes-1.0.0-r2.zip').waitFor();
  await release.getByText('Your release · ready to save').waitFor();
  await release.getByRole('button',{name:'Save'}).waitFor();
  if(await view.locator('.export-row.released').count()!==1)throw new Error('One release is marked');
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-released.png')});
  if(await cta.count())throw new Error('Nothing left to release after the release');
  // From the Overview, nothing changed since (a change undone): Exports opens on the ZIP already there.
  await view.evaluate(()=>{const f=window.conversionFixture;f.changes={count:3,sinceZip:1,changedSinceZip:false};f.project.updatedAt=new Date().toISOString();f.emit('project-updated',structuredClone(f.project))});
  await view.getByRole('tab',{name:'Overview'}).click();
  await view.locator('.changes-line').getByRole('button',{name:'Make release'}).click();
  await view.getByText('Nothing changed since the last release: clara-hayes-1.0.0-r2.zip is ready to save.').waitFor();
  await view.locator('.export-row.released',{hasText:'clara-hayes-1.0.0-r2.zip'}).waitFor();
  if(await view.evaluate(()=>window.conversionFixture.packaged)!==2)throw new Error('Each Make release asks the host once');
  const deleteOld=view.getByRole('button',{name:'Delete clara-hayes-1.0.0.zip from revision 1'});
  await deleteOld.waitFor();
  await view.evaluate(()=>{window.conversionFixture.confirmNext=false});
  await deleteOld.click();
  if(await view.evaluate(()=>window.conversionFixture.calls.some(c=>c.command==='delete_previous_artifact')))throw new Error('Cancelling must keep the previous version');
  await deleteOld.click();
  await view.waitForFunction(()=>window.conversionFixture.calls.some(c=>c.command==='delete_previous_artifact'&&c.args.artifactId==='a-theme'));
  if(await deleteOld.count())throw new Error('Only the confirmed previous version should disappear');
  await release.getByText('clara-hayes-1.0.0-r2.zip').waitFor();
  // Start over from the original: only in the project menu, for when something broke, behind its warning.
  await view.evaluate(()=>{const f=window.conversionFixture;f.changes={count:3,sinceZip:1,changedSinceZip:true};f.project.updatedAt=new Date().toISOString();f.emit('project-updated',structuredClone(f.project))});
  await view.getByRole('tab',{name:'Overview'}).click();
  await view.locator('.changes-line',{hasText:'Changes since last ZIP: 1'}).waitFor();
  await heading.locator('summary[aria-label="Manage project"]').click();
  await heading.getByRole('button',{name:'Start over from the original…'}).click();
  const startOver=view.getByRole('alertdialog',{name:'Start over from the original?'});
  await startOver.getByText('This converts the site again from its original source. All changes made after delivery (3) are lost and the current theme is replaced. Use this only if something is broken.',{exact:false}).waitFor();
  await startOver.getByText('The last change is in no ZIP yet: use Make release first to keep it in a ZIP.',{exact:false}).waitFor();
  if(await startOver.getByRole('button',{name:'Start over'}).isEnabled())throw new Error('Start over needs the explicit confirmation first');
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-startover.png')});
  await startOver.getByRole('button',{name:'Keep the delivered theme'}).click();
  if(await view.evaluate(()=>window.conversionFixture.run?.mode==='full'))throw new Error('Keeping the theme starts nothing');
  await heading.locator('summary[aria-label="Manage project"]').click();
  await heading.getByRole('button',{name:'Start over from the original…'}).click();
  await startOver.getByRole('radio',{name:/^Full/}).check();
  await startOver.getByRole('checkbox',{name:/I understand/}).check();
  await startOver.getByRole('button',{name:'Start over'}).click();
  await view.waitForFunction(()=>window.conversionFixture.run?.mode==='full');
  await view.evaluate(()=>window.conversionFixture.emit('turn-completed',{projectId:'fixture'}));

  // Gutenberg from an HTML theme keeps its own Start conversion and nine steps.
  await view.getByRole('navigation',{name:'Projects'}).getByRole('button',{name:'Clara theme'}).click();
  await heading.getByRole('heading',{name:'Clara theme'}).waitFor();
  if(await heading.getByRole('button',{name:'Flash',exact:true}).count())throw new Error('Flash belongs to site projects only');
  await view.evaluate(()=>{window.conversionFixture.h2g={current:3,started:new Date(Date.now()-42*60000).toISOString(),steps:{'3':{at:new Date().toISOString(),note:'Inter and Lora kept local'}}}});
  await heading.getByRole('button',{name:'Start conversion'}).click();
  await view.waitForFunction(()=>window.conversionFixture.run?.mode==='start'&&window.conversionFixture.run.projectId==='theme-project');
  await view.evaluate(()=>{const f=window.conversionFixture;f.emit('turn-started',{projectId:'theme-project'});f.emit('project-updated',structuredClone(f.theme))});
  await view.getByText('Inter and Lora kept local').waitFor();
  await view.locator('.pipeline li.current',{hasText:'Fonts'}).waitFor();
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot});

  // Delivered: Preview and Compare as for an HTML theme, a message is a change, Make release packs it.
  await view.evaluate(()=>{const f=window.conversionFixture;const at=new Date().toISOString();
   f.theme.phase='deliverable_ready';f.theme.artifacts=[{id:'g1',revision:1,filename:'clara-blocks.zip',sha256:'z',createdAt:at,kind:'theme',reviewed:true,checks:'h2g'}];
   f.h2g={...f.h2g,current:9,changes:{count:null,sinceZip:null,changedSinceZip:true}};
   f.review={capturedAt:at,preview:'http://127.0.0.1:8899',pages:[{key:'front-page',title:'front-page',page:null,route:'/',desktop:{image:'compare/desktop/front-page-converted.png',original:'compare/desktop/front-page-original.png',diffPercent:0.8,origHeight:null,wpHeight:null,error:null},mobile:null}]};
   f.emit('turn-completed',{projectId:'theme-project'});f.theme.updatedAt=at;f.emit('project-updated',structuredClone(f.theme))});
  await view.locator('.release-cta').getByText('The theme changed since the last release.').waitFor();
  await view.getByRole('tab',{name:'Exports',selected:true}).waitFor();
  await view.getByRole('tab',{name:'Overview'}).click();
  await view.locator('.changes-line',{hasText:'The theme changed since the last release.'}).getByRole('button',{name:'Make release'}).waitFor();
  if(await heading.getByRole('button',{name:'Continue'}).count())throw new Error('A delivered Gutenberg theme is not converted again from the header');
  if(await view.getByRole('textbox',{name:'Message your assistant'}).getAttribute('placeholder')!=='Ask for a change: it is made in the live preview, no rebuild…')throw new Error('A message on a delivered Gutenberg theme is a change');
  await view.getByRole('tab',{name:'Preview'}).waitFor();
  await view.getByRole('tab',{name:'Compare'}).click();
  await view.locator('.comparison-labels',{hasText:'Original HTML theme'}).getByText('Gutenberg conversion').waitFor();
  await view.locator('.compare-meta',{hasText:'0.80% different'}).waitFor();
  if(!(await view.evaluate(()=>['compare/desktop/front-page-converted.png','compare/desktop/front-page-original.png'].every(p=>window.conversionFixture.shownImages.includes(p)))))throw new Error('Both pages of the pair are loaded');
  if(bundle.screenshot)await view.screenshot({path:bundle.screenshot.replace('.png','-h2g-compare.png')});

  if(await view.evaluate(()=>window.conversionFixture.calls.filter(c=>c.command==='send_message').length)!==1)throw new Error('Only the owner\'s one change was sent');
  const unexpected=await view.evaluate(()=>window.conversionFixture.calls.filter(c=>['flash_convert','html_convert','build_theme','live_fix','review_page','create_report','decide_convergence'].includes(c.command)).map(c=>c.command));
  if(unexpected.length)throw new Error('The UI called removed or unexpected host commands: '+unexpected.join(', '));
  if(pageErrors.length)throw new Error(pageErrors.join('\n'));
 }finally{await view.close()}
}
