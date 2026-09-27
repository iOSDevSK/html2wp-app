#!/usr/bin/env node
// Keep the desktop product version aligned across its four package formats.
// Runtime adapter and plugin versions are separate compatibility identities.
import {readFileSync,writeFileSync} from 'node:fs';

const version=process.argv[2];
if(version!=='--check'&&!/^\d+\.\d+\.\d+$/.test(version||'')){
  throw Error('Use: node scripts/version.mjs --check | <major.minor.patch>');
}
const read=path=>readFileSync(path,'utf8');
const json=path=>JSON.parse(read(path));
const write=(path,data)=>writeFileSync(path,JSON.stringify(data,null,2)+'\n');
const packageMeta=json('package.json');
const lock=json('package-lock.json');
const tauri=json('src-tauri/tauri.conf.json');
const notice=json('notices/desktop.cdx.json');
const cargo=read('src-tauri/Cargo.toml');
const cargoLock=read('src-tauri/Cargo.lock');
const packageLockPattern=/\[\[package\]\]\nname = "html2wp-desktop"\nversion = "([^"]+)"/;
const cargoPattern=/^version = "([^"]+)"/m;
const cargoVersion=cargo.match(cargoPattern)?.[1];
const cargoLockedVersion=cargoLock.match(packageLockPattern)?.[1];
if(!cargoVersion||!cargoLockedVersion)throw Error('Cannot identify the desktop Rust package version');
if(version==='--check'){
  const versions={package:packageMeta.version,packageLock:lock.version,packageLockRoot:lock.packages[''].version,
    tauri:tauri.version,cargo:cargoVersion,cargoLock:cargoLockedVersion,notice:notice.metadata.component.version};
  if(new Set(Object.values(versions)).size!==1)throw Error('Desktop version mismatch: '+JSON.stringify(versions));
  console.log(`Desktop version ${packageMeta.version}: all package metadata matches; runtime adapter ${json('runtime/versions.json').adapterVersion} is independent.`);
}else{
  packageMeta.version=version;lock.version=version;lock.packages[''].version=version;
  tauri.version=version;notice.metadata.component.version=version;
  write('package.json',packageMeta);write('package-lock.json',lock);
  write('src-tauri/tauri.conf.json',tauri);write('notices/desktop.cdx.json',notice);
  writeFileSync('src-tauri/Cargo.toml',cargo.replace(cargoPattern,`version = "${version}"`));
  writeFileSync('src-tauri/Cargo.lock',cargoLock.replace(packageLockPattern,`[[package]]\nname = "html2wp-desktop"\nversion = "${version}"`));
  console.log(`Desktop product set to ${version}; runtime adapter unchanged.`);
}
