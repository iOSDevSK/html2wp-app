import {describe,it,expect} from 'vitest';
import {exportFilter,exportOrder,exportTitle,runLabel} from './exports';
import type {Artifact} from './types';

const a=(kind:Artifact['kind'],filename:string,checks?:string):Artifact=>({id:filename,revision:1,filename,sha256:'x',createdAt:'',kind,reviewed:true,checks});
describe('exports',()=>{
 it('lists whatever the run delivered, the theme first and the report next',()=>{
  const files=[a('file','notes.md'),a('pdf','report.pdf'),a('theme','site.zip'),a('report','CONVERSION-REPORT.md')];
  expect(exportOrder(files).map(f=>f.filename)).toEqual(['site.zip','report.pdf','CONVERSION-REPORT.md','notes.md']);
  expect(exportOrder([])).toEqual([]);
 });
 it('names every kind in English and saves each file with its own extension',()=>{
  expect(exportTitle.theme).toBe('WordPress theme');
  expect(exportTitle.file).toBe('Delivered file');
  expect(exportFilter(a('theme','site.zip'))).toEqual({name:'WordPress theme',extensions:['zip']});
  expect(exportFilter(a('pdf','report.pdf'))).toEqual({name:'PDF report',extensions:['pdf']});
  expect(exportFilter(a('summary','verification.md'))).toEqual({name:'Verification summary',extensions:['md']});
  expect(exportTitle.report).toBe('Conversion report');
  expect(exportTitle.pdf).toBe('Conversion report (PDF)');
  expect(exportFilter(a('report','CONVERSION-REPORT.md'))).toEqual({name:'Conversion report',extensions:['md']});
  expect(exportFilter(a('file','README'))).toEqual({name:'File',extensions:['*']});
 });
 it('says which run made a file',()=>{
  expect(runLabel(a('theme','x.zip','flash'))).toBe('Flash conversion');
  expect(runLabel(a('theme','x.zip','full'))).toBe('Full conversion');
  expect(runLabel(a('theme','x.zip','h2g'))).toBe('Gutenberg from an HTML theme');
  expect(runLabel(a('theme','x-r2.zip','packaged'))).toBe('Packaged after changes, not checked again');
  expect(runLabel(a('theme','x.zip','quick'))).toBe('Earlier release');
 });
});
