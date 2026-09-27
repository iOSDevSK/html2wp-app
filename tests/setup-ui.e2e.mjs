import {test} from '@playwright/test';
import {readFileSync,readdirSync} from 'node:fs';
import checkSetupUi from './setup-ui.browser.js';

test('environment feedback and recoverable Codex sign-in',async({page})=>{
 const assets='dist/assets';
 const read=(suffix)=>readFileSync(`${assets}/${readdirSync(assets).find(file=>file.endsWith(suffix))}`,'utf8');
 await checkSetupUi(page,{js:read('.js'),css:read('.css')});
});
