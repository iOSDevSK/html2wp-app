/** The plugin's page-by-page comparison (compare::index, APP-CONTRACT v1.2 §6b): the source beside WordPress. */
/** One width of a page: the plugin's side-by-side image, or (Gutenberg from an HTML theme) the new page with the `original` beside it. */
export type CompareView={referenceKind?:string;referenceNotice?:string|null;image:string|null;original?:string|null;diffPercent:number|null;origHeight:number|null;wpHeight:number|null;error:string|null};
export type ComparePage={key:string;title:string|null;page:string|null;route:string|null;desktop:CompareView|null;mobile:CompareView|null};
export type CompareIndex={capturedAt?:string|null;preview?:string|null;pages?:ComparePage[]};
/** Its status as the plugin writes it, whether the app runs one now, and why the last could not run. */
export type CompareStatus={state:'running'|'done'|'failed'|null;note:string|null;startedAt:string|null;updatedAt:string|null;running:boolean;error:string|null};
export type Width='desktop'|'mobile';
export const widths:{width:Width;label:string}[]=[{width:'desktop',label:'Desktop · 1440'},{width:'mobile',label:'Mobile · 390'}];

/** How different a view is, as the plugin measured it; a hint, never a verdict. */
export function diffLabel(diff:number|null|undefined):string{return typeof diff==='number'?`${diff.toFixed(2)}% different`:'not measured'}
/** The page's address as the owner reads it: the path of its WordPress URL. */
export function routeOf(page:Pick<ComparePage,'route'|'page'>):string|null{
 if(!page.route)return page.page;
 try{return new URL(page.route).pathname}catch{return page.route}
}
/** A page in the list: its title, its route and its difference at this width. */
export function pageLabel(page:ComparePage,width:Width='desktop'):string{
 const view=page[width];
 const name=page.title||page.key;
 const measure=!view||view.error||!view.image?'not captured':typeof view.diffPercent==='number'?`${view.diffPercent.toFixed(2)}%`:'';
 const route=routeOf(page);
 return [name,route&&route!==name?route:null,measure].filter(Boolean).join(' · ');
}
/** The widths this comparison has any image for. */
export function widthsOf(pages:ComparePage[]):Width[]{return widths.map(w=>w.width).filter(w=>pages.some(p=>p[w]?.image))}
