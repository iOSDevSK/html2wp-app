import {describe,it,expect} from 'vitest';
import review from '../tests/fixtures/visual-review/visual-compare.json';
import {diffLabel,pageLabel,routeOf,widthsOf,type ComparePage,type CompareView} from './comparison';

// The fixture index as compare::index hands it over (every field present).
const view=(v:{image?:string;diffPercent?:number;origHeight?:number;wpHeight?:number;error?:string}|undefined):CompareView|null=>v?{image:v.image??null,diffPercent:v.diffPercent??null,origHeight:v.origHeight??null,wpHeight:v.wpHeight??null,error:v.error??null}:null;
const pages:ComparePage[]=review.pages.map(p=>({key:p.key,title:p.title,page:p.page,route:p.route,desktop:view(p.desktop),mobile:view(p.mobile)}));
describe('comparison',()=>{
 it('names each page by its title, route and the difference at the chosen width',()=>{
  expect(pages.map(p=>pageLabel(p))).toEqual(['Home · / · 0.42%','About · /about/ · 3.70%','Contact · /contact/ · not captured']);
  expect(pages.map(p=>pageLabel(p,'mobile'))).toEqual(['Home · / · 1.30%','About · /about/ · 5.10%','Contact · /contact/ · not captured']);
  expect(diffLabel(0.42)).toBe('0.42% different');
  expect(diffLabel(null)).toBe('not measured');
 });
 it('reads the route as a path and offers only the widths it has images for',()=>{
  expect(routeOf({route:'http://localhost:53412/about/',page:'about.html'})).toBe('/about/');
  expect(routeOf({route:null,page:'about.html'})).toBe('about.html');
  expect(widthsOf(pages)).toEqual(['desktop','mobile']);
  expect(widthsOf(pages.map(p=>({...p,mobile:null})))).toEqual(['desktop']);
  expect(widthsOf([])).toEqual([]);
 });
});
