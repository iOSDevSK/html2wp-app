// Runs against production assets, with deterministic native IPC fixtures.
// No Docker, account, sign-in page or model call is used.
export default async function checkSetupUi(page, bundle) {
  const view = await page.context().newPage();
  try {
    await view.setViewportSize({width:1380,height:900});
    await view.setContent('<html><head></head><body><div id="root"></div></body></html>');
    await view.evaluate(() => {
      const fixture = window.setupFixture = {calls:[],callbacks:{},events:{},next:1,holdCheck:false,account:null,model:'',efforts:{}};
      const ready = {ready:true,docker:true,imageReady:true,version:'fixture',message:'Your local environment is ready.'};
      fixture.ready = ready;
      fixture.models = [
        {id:'a',model:'fixture-a',displayName:'Fixture A',isDefault:true,defaultReasoningEffort:'low',supportedReasoningEfforts:[{reasoningEffort:'low',description:'Quick fixture'},{reasoningEffort:'high',description:'Thorough fixture'}]},
        {id:'b',model:'fixture-b',displayName:'Fixture B',isDefault:false,defaultReasoningEffort:'medium',supportedReasoningEfforts:[{reasoningEffort:'medium',description:'Balanced fixture'},{reasoningEffort:'xhigh',description:'Extended fixture'}]}
      ];
      window.isTauri = true;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener:()=>{}};
      window.__TAURI_INTERNALS__ = {
        transformCallback: fn => { const id=fixture.next++;fixture.callbacks[id]=fn;return id; },
        unregisterCallback: id => {delete fixture.callbacks[id]},
        invoke: async (command,args={}) => {
          fixture.calls.push({command,args});
          if(command==='plugin:event|listen'){fixture.events[args.event]=args.handler;return args.handler}
          if(command==='plugin:event|unlisten')return;
          if(command==='get_bootstrap')return {projects:[],versions:{pluginVersion:'fixture',pluginCommit:'fixture',codexVersion:'0.154.0',adapterVersion:'0.1.0'},platform:'macos',architecture:'aarch64',disclosureAccepted:true,licenceConfigured:false,lastUpdateCheck:null,activeProject:null};
          if(command==='check_runtime')return fixture.holdCheck?new Promise((resolve,reject)=>{fixture.checkResolve=resolve;fixture.checkReject=reject}):ready;
          if(command==='prepare_runtime')return new Promise(resolve=>{fixture.prepareResolve=resolve});
          if(command==='cancel_setup')return;
          if(command==='account_read')return {account:fixture.account};
          if(command==='licence_status')return {mode:'free',state:'free',message:'Fixture Free account'};
          if(command==='account_login')return new Promise((resolve,reject)=>{fixture.loginResolve=resolve;fixture.loginReject=reject});
          if(command==='account_cancel'||command==='open_url')return;
          if(command==='model_catalog')return {models:fixture.models,selectedModel:fixture.model,selectedEffort:fixture.efforts[fixture.model||'fixture-a']||''};
          if(command==='set_active_project')return;
          if(command==='chrome_bridge_status')return fixture.bridge||(fixture.bridge={paired:false,code:'482913',port:47811});
          if(command==='chrome_bridge_regenerate')return fixture.bridge={...fixture.bridge,code:'105726'};
          if(command==='chrome_bridge_unpair')return fixture.bridge={...fixture.bridge,paired:false};
          if(command==='set_max_parallel'){if(args.value<1||args.value>4)throw Error('out of range');fixture.maxParallel=args.value;return args.value}
          if(command==='select_model'){fixture.model=args.model;return {selectedModel:args.model,selectedEffort:fixture.efforts[args.model||'fixture-a']||''}}
          if(command==='select_effort'){if(args.model!==fixture.model)throw Error('Stale model');fixture.efforts[args.model||'fixture-a']=args.effort;return}
          throw new Error('Unexpected fixture command: '+command);
        }
      };
    });
    await view.addStyleTag({content:bundle.css});
    await view.addScriptTag({content:bundle.js,type:'module'});
    await view.getByRole('button',{name:/Settings/}).click();
    const check = view.getByRole('button',{name:'Check environment',exact:true});
    await check.waitFor();
    await view.evaluate(()=>{window.setupFixture.holdCheck=true});
    await check.click();
    const checking = view.getByRole('button',{name:'Checking…',exact:true});
    await checking.waitFor();
    if(!await checking.isDisabled())throw Error('Check must disable while pending');
    await view.getByText('Checking your environment…',{exact:true}).waitFor();
    await view.evaluate(()=>window.setupFixture.checkResolve(window.setupFixture.ready));
    await view.getByText('Environment checked. Docker and conversion tools are ready.',{exact:true}).waitFor();
    await view.getByText(/Last checked at/).waitFor();
    await view.getByRole('button',{name:'Repair environment',exact:true}).click();
    const preparing=view.getByRole('button',{name:'Preparing…',exact:true});
    await preparing.waitFor();
    if(!await preparing.isDisabled())throw Error('Rebuild must disable while pending');
    await view.evaluate(()=>window.setupFixture.prepareResolve(window.setupFixture.ready));
    await view.getByRole('button',{name:'Repair environment',exact:true}).waitFor();
    await check.click();
    await view.evaluate(()=>window.setupFixture.checkResolve({ready:false,docker:false,imageReady:false,message:'Docker is stopped (fixture).'}));
    await view.getByText('Docker is stopped (fixture).',{exact:true}).first().waitFor();
    if(!await view.getByRole('button',{name:'Connect with ChatGPT',exact:true}).isDisabled())throw Error('Login must be disabled when Docker is unavailable');
    await view.getByRole('button',{name:'Prepare environment',exact:true}).click();
    await view.evaluate(()=>{
      const f=window.setupFixture;
      f.callbacks[f.events['runtime-progress']]({event:'runtime-progress',payload:{phase:'download',message:'Downloading Docker Desktop…',percent:42,cancellable:true}});
    });
    await view.getByText('Downloading Docker Desktop…',{exact:true}).waitFor();
    if(await view.getByRole('progressbar').getAttribute('value')!=='42')throw Error('Download progress not rendered');
    await view.getByRole('button',{name:'Stop setup',exact:true}).click();
    await view.getByRole('button',{name:'Stopping…',exact:true}).waitFor();
    await view.evaluate(()=>window.setupFixture.prepareResolve(window.setupFixture.ready));
    await view.getByRole('button',{name:'Repair environment',exact:true}).waitFor();
    await check.click();
    await view.evaluate(()=>window.setupFixture.checkResolve(window.setupFixture.ready));
    const connect=view.getByRole('button',{name:'Connect with ChatGPT',exact:true});
    await connect.click();
    await view.getByRole('button',{name:'Starting sign-in…',exact:true}).waitFor();
    await view.evaluate(()=>window.setupFixture.loginReject('Codex could not start (fixture). Update the app.'));
    await view.locator('.setup-error').getByText('Codex could not start (fixture). Update the app.',{exact:true}).waitFor();
    const toastClose=view.getByRole('button',{name:'Dismiss notification'});
    if(await toastClose.count())await toastClose.click();
    if(!await view.locator('.setup-error').isVisible())throw Error('Connection error disappeared with toast');
    await connect.click();
    await view.evaluate(()=>window.setupFixture.loginResolve({loginId:'fixture-login',verificationUrl:'https://auth.openai.com/codex/device',userCode:'FIXTURE-CODE'}));
    await view.getByText('FIXTURE-CODE',{exact:true}).waitFor();
    const waiting=view.getByRole('button',{name:'Waiting for sign-in…',exact:true});
    if(!await waiting.isDisabled())throw Error('Duplicate sign-in was allowed');
    if(await view.locator('.setup-error').count())throw Error('Retry must clear old error');
    await view.getByRole('button',{name:'Cancel',exact:true}).click();
    await connect.waitFor();
    await connect.click();
    await view.evaluate(()=>window.setupFixture.loginResolve({loginId:'fixture-login-2',verificationUrl:'https://auth.openai.com/codex/device',userCode:'FIXTURE-SECOND'}));
    await view.getByText('FIXTURE-SECOND',{exact:true}).waitFor();
    await view.evaluate(()=>{
      const f=window.setupFixture;f.account={type:'chatgpt',email:'fixture@example.invalid',planType:'Fixture'};
      f.callbacks[f.events['account-event']]({event:'account-event',payload:{type:'loginCompleted',success:true}});
    });
    await view.getByRole('button',{name:'Sign out',exact:true}).waitFor();
    if(await view.locator('.login-code').count())throw Error('Completed sign-in code remained visible');
    const model=view.getByLabel('Conversion model',{exact:true});
    const effort=view.getByLabel('Reasoning effort',{exact:true});
    await model.selectOption('fixture-a');
    await effort.selectOption('high');
    await view.waitForFunction(()=>document.querySelector('#reasoning-effort').value==='high'&&!document.querySelector('#reasoning-effort').disabled);
    // Parallel conversions: the owner picks the limit, the backend saves it.
    const parallel=view.getByLabel('Conversions at once',{exact:true});
    if(await parallel.inputValue()!=='2')throw Error('The default parallel limit is not 2');
    if(await parallel.locator('option').count()!==6)throw Error('The parallel limit must offer 1 to 6');
    await parallel.selectOption('3');
    await view.waitForFunction(()=>window.setupFixture.maxParallel===3);
    await view.waitForFunction(()=>document.querySelector('#max-parallel').value==='3');
    // Chrome extension: a pairing code, a new code on request, the paired state from the app.
    const pairing=view.getByLabel('Pairing code',{exact:true});
    await view.waitForFunction(()=>document.querySelector('.pairing-code')?.textContent==='482 913');
    await view.getByRole('button',{name:'New code',exact:true}).click();
    await view.waitForFunction(()=>document.querySelector('.pairing-code')?.textContent==='105 726');
    if(!await pairing.isVisible())throw Error('The pairing code is not shown');
    await view.evaluate(()=>{const f=window.setupFixture;f.bridge={...f.bridge,paired:true};f.callbacks[f.events['chrome-bridge']]({event:'chrome-bridge',payload:f.bridge})});
    await view.getByText('Paired',{exact:true}).waitFor();
    await view.getByRole('button',{name:'Unpair',exact:true}).click();
    await view.getByText('The Chrome extension is unpaired. Pair it again with a new code.',{exact:true}).waitFor();
    await view.getByRole('button',{name:'Refresh Codex models',exact:true}).click();
    await view.waitForFunction(()=>document.querySelector('#reasoning-effort').value==='high'&&!document.querySelector('#reasoning-effort').disabled);
    await model.selectOption('fixture-b');
    await view.waitForFunction(()=>document.querySelector('#reasoning-effort').value===''&&!document.querySelector('#reasoning-effort').disabled);
    if(await effort.locator('option[value="high"]').count())throw Error('Effort from another model leaked into the options');
    await effort.selectOption('xhigh');
    await view.waitForFunction(()=>document.querySelector('#reasoning-effort').value==='xhigh'&&!document.querySelector('#reasoning-effort').disabled);
    await model.selectOption('fixture-a');
    await view.waitForFunction(()=>document.querySelector('#reasoning-effort').value==='high'&&!document.querySelector('#reasoning-effort').disabled);
    await effort.selectOption('');
    await view.waitForFunction(()=>document.querySelector('#reasoning-effort').value===''&&!document.querySelector('#reasoning-effort').disabled);
    const saved=await view.evaluate(()=>window.setupFixture.efforts);
    if(saved['fixture-a']!==''||saved['fixture-b']!=='xhigh')throw Error('Per-model reasoning preferences were not saved');

    // A dropped Codex process is not a sign-out: the app reconnects by itself.
    const readsBefore=await view.evaluate(()=>{
      const f=window.setupFixture;f.account=null;
      const before=f.calls.filter(c=>c.command==='account_read').length;
      f.callbacks[f.events['account-event']]({event:'account-event',payload:{type:'disconnected',error:'Codex disconnected (fixture). Connect again.'}});
      return before;
    });
    await view.waitForFunction(before=>window.setupFixture.calls.filter(c=>c.command==='account_read').length>before,readsBefore,{timeout:8000});
    if(await view.locator('.setup-error').getByText('Codex disconnected (fixture). Connect again.',{exact:true}).count())throw Error('A reconnectable disconnect was shown as an error');
    await connect.waitFor();
    const result=await view.evaluate(()=>({
      checks:window.setupFixture.calls.filter(c=>c.command==='check_runtime').length,
      rebuilds:window.setupFixture.calls.filter(c=>c.command==='prepare_runtime').length,
      loginAttempts:window.setupFixture.calls.filter(c=>c.command==='account_login').length,
      openedSignInPages:window.setupFixture.calls.filter(c=>c.command==='open_url').length,
      noHorizontalOverflow:document.documentElement.scrollWidth<=innerWidth
    }));
    if(result.checks!==4||result.rebuilds!==2||result.loginAttempts!==3||result.openedSignInPages!==2||!result.noHorizontalOverflow)throw Error(JSON.stringify(result));
    return {passed:true,reasoningEffort:"supported options, persistence, model switching and default reset passed",chromeExtension:"pairing code, new code, paired state and unpair",...result};
  } finally {await view.close()}
}
