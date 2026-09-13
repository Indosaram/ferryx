import React from 'react';
import { createRoot } from 'react-dom/client';
import '/ui/src/index.css';
import '/ui/src/settings-runtime.css';
localStorage.setItem('ferryx_remote_token', 'qa-isolated-device');
import { RemoteApp } from '/ui/src/remote/RemoteApp.tsx';
window.qaWait = (selector, text) => new Promise((resolve, reject) => {
  const check = () => { const el = document.querySelector(selector); if (el && (text === undefined || el.textContent.includes(text))) { observer.disconnect(); clearTimeout(timeout); resolve(true); } };
  const observer = new MutationObserver(check);
  const timeout = setTimeout(() => { observer.disconnect(); reject(new Error(`Missing ${selector}: ${text}`)); }, 8000);
  observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true }); check();
});
createRoot(document.getElementById('root')).render(<RemoteApp />);
