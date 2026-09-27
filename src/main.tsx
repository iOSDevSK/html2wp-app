import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import './styles.css';
// macOS: the window's own title bar (the native one is hidden, its buttons
// stay), dark with a white title in dark mode and light in light mode.
const mac=/Mac/.test(navigator.platform);
if(mac)document.documentElement.classList.add('custom-titlebar');
ReactDOM.createRoot(document.getElementById('root')!).render(<React.StrictMode>{mac&&<div className="titlebar" data-tauri-drag-region><span data-tauri-drag-region>html2wp</span></div>}<App/></React.StrictMode>);
