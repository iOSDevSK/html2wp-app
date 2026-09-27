import {readFile,readdir,mkdir,copyFile,writeFile} from 'node:fs/promises';
import {existsSync} from 'node:fs';
import path from 'node:path';
const root='notices';await mkdir(`${root}/texts`,{recursive:true});
const components=[];const missing=[];
async function component(ecosystem,name,version,license,dir,source){
 const ref=`pkg:${ecosystem}/${name}@${version}`;
 const c={type:'library',name,version,'bom-ref':ref,purl:ref,externalReferences:source?[{type:'distribution',url:source}]:[]};
 if(license)c.licenses=[{expression:license.replace(/^(MIT|Apache-2\.0)\/(MIT|Apache-2\.0)$/, '$1 OR $2')}];
 components.push(c);
 const target=path.join(root,'texts',`${ecosystem}-${name.replaceAll('/','_')}@${version}`);await mkdir(target,{recursive:true});
 const isNotice=f=>/^(licen[sc]e|copying|notice|copyright)([._-]|$)/i.test(f);
 const files=dir&&existsSync(dir)?(await readdir(dir)).filter(isNotice):[];
 if(!files.length){
  // Some registry archives omit their upstream licence files. Keep reviewed,
  // version-specific copies in texts/ available for repeatable regeneration.
  if(!(await readdir(target)).some(isNotice))missing.push(ref);
  return;
 }
 for(const file of files){try{await copyFile(path.join(dir,file),path.join(target,file))}catch{/* Directories are not licence texts. */}}
}
const lock=JSON.parse(await readFile('package-lock.json','utf8'));
for(const [location,p] of Object.entries(lock.packages)){
 if(!location||p.dev)continue;
 const name=p.name||location.split('node_modules/').at(-1);
 await component('npm',name,p.version,typeof p.license==='string'?p.license:null,location,p.resolved);
}
const metadata=JSON.parse(await readFile('.cache/cargo-metadata.json','utf8'));
for(const p of metadata.packages){if(!p.source)continue;await component('cargo',p.name,p.version,p.license,path.dirname(p.manifest_path),`https://crates.io/api/v1/crates/${p.name}/${p.version}/download`);}
await writeFile(`${root}/desktop.cdx.json`,JSON.stringify({bomFormat:'CycloneDX',specVersion:'1.6',version:1,metadata:{timestamp:new Date().toISOString(),component:{type:'application',name:'html2wp Desktop',version:lock.version},properties:[{name:'inventory:target',value:'aarch64-apple-darwin'},{name:'inventory:scope',value:'Locked desktop dependencies; may include test/build dependencies. Runtime image inventory is separate.'}]},components},null,2)+'\n');
await writeFile(`${root}/THIRD_PARTY_NOTICES.md`,`# Third-party desktop dependencies\n\nGenerated from package-lock.json and filtered Cargo metadata. Exact upstream licence and notice files are under texts/. The CycloneDX inventory is desktop.cdx.json. Runtime OS packages, Chromium and downloaded WordPress images require a separate image inventory before redistribution.\n\nPackages with no top-level notice file in this checkout (review their distribution sources before publishing):\n\n${missing.map(x=>'- '+x).join('\n')}\n`);
console.log(`Inventoried ${components.length} desktop components; ${missing.length} require source notice review.`);
