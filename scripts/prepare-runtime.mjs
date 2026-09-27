import { cp, mkdir, rm } from 'node:fs/promises';
// The html2wp plugin is not part of the runtime: the app fetches it from
// GitHub and mounts it at /opt/html2wp. An old copy here would ship in the app.
await rm('runtime/plugin',{recursive:true,force:true});
// Gutenberg from an HTML theme: the pinned skill, where the app points Codex
// for skills (skills/extraRoots in the image: /opt/desktop/skills).
await mkdir('runtime/skills',{recursive:true});
await cp('vendor/html2wp-to-gutenberg','runtime/skills/html2wp-to-gutenberg',{recursive:true,filter:source=>!/[\\/]\.git($|[\\/])/.test(source)&&!source.includes('__pycache__')&&!source.endsWith('.pyc')});
console.log('Prepared the runtime build context (html2wp-to-gutenberg).');
