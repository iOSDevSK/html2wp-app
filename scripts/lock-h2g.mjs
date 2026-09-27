import {readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
// Gutenberg from an HTML theme: the pinned html2wp-to-gutenberg skill, locked
// file by file like the html2wp plugin (--upstream takes the submodule's commit).
const root='vendor/html2wp-to-gutenberg';
const versions=JSON.parse(await readFile('runtime/versions.json','utf8'));
if(process.argv.includes('--upstream')){versions.h2gCommit=execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim();await writeFile('runtime/versions.json',JSON.stringify(versions,null,2)+'\n')}
const hashes={};
async function walk(dir=''){for(const e of(await readdir(dir?`${root}/${dir}`:root,{withFileTypes:true})).sort((a,b)=>a.name.localeCompare(b.name))){const p=dir?`${dir}/${e.name}`:e.name;if(e.name==='.git'||e.name==='__pycache__'||e.name.endsWith('.pyc'))continue;if(e.isDirectory())await walk(p);else if(e.isFile())hashes[p]=createHash('sha256').update(await readFile(`${root}/${p}`)).digest('hex');else throw Error('The skill contains a non-regular file');}}
await walk();await writeFile('runtime/h2g-files.sha256.json',JSON.stringify(hashes,null,2)+'\n');console.log(`Locked ${Object.keys(hashes).length} html2wp-to-gutenberg files.`);
