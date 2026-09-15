import { createServer } from "../../../ui/node_modules/vite";
import react from "../../../ui/node_modules/@vitejs/plugin-react";
import { resolve } from "node:path";
import { mkdir } from "node:fs/promises";
import net from "node:net";

const root = resolve(import.meta.dir, "../../..");
const outDir = resolve(import.meta.dir);
const screenshotDir = resolve(outDir, "screenshots");
await mkdir(screenshotDir, { recursive: true });

// Allocate a dynamic free port that is guaranteed not 5173
const netServer = net.createServer();
await new Promise(r => netServer.listen(0, "127.0.0.1", r));
const freePort = netServer.address().port;
netServer.close();

console.log(`[QA Harness] Ephemeral port allocated: ${freePort} (isolated, not 5173)`);

// Load Tailwind and config to render exact product CSS
const tailwind = (await import("../../../ui/node_modules/tailwindcss")).default;
const autoprefixer = (await import("../../../ui/node_modules/autoprefixer")).default;
const tailwindConfig = (await import("../../../ui/tailwind.config.js")).default;

const vite = await createServer({
  configFile: false,
  root,
  plugins: [react()],
  css: {
    postcss: {
      plugins: [
        tailwind({
          ...tailwindConfig,
          content: [
            resolve(root, "ui/index.html"),
            resolve(root, "ui/src/**/*.{js,ts,jsx,tsx}"),
            resolve(root, "docs/evidence/remote-password-20260915/**/*.{html,tsx}"),
          ],
        }),
        autoprefixer(),
      ],
    },
  },
  resolve: {
    alias: {
      "@": resolve(root, "ui/src"),
      react: resolve(root, "ui/node_modules/react"),
      "react-dom": resolve(root, "ui/node_modules/react-dom"),
    },
  },
  server: {
    host: "127.0.0.1",
    port: freePort,
    strictPort: true,
    hmr: false,
  },
});

await vite.listen();
const harnessUrl = `http://127.0.0.1:${freePort}/docs/evidence/remote-password-20260915/index.html`;
console.log(`[QA Harness] Vite server ready at: ${harnessUrl}`);


const evaluate = (view, code) => view.evaluate(`eval(${JSON.stringify(code)})`);
const results = { fixtureOnly: true, checks: [], screenshots: [], errors: [] };
const check = (vp, name, pass, detail) => {
  results.checks.push({vp, name, pass: Boolean(pass), detail});
  console.log(`${pass ? "PASS" : "FAIL"} ${vp} ${name}`, detail ?? "");
};
// MutationObserver subscribes BEFORE action. Timeout is a failure bound, not a delay.
async function transition(view, predicate, action = "") {
  return evaluate(view,`new Promise((resolve, reject) => {
    let timer;
    const finish = () => { if (${predicate}) { observer.disconnect(); clearTimeout(timer); resolve(true); } };
    const observer = new MutationObserver(finish);
    observer.observe(document, {subtree:true, childList:true, attributes:true, characterData:true});
    timer = setTimeout(() => { observer.disconnect(); reject(new Error('DOM state timeout: '+${JSON.stringify(predicate)})); }, 8000);
    ${action}; finish();
  })`);
}
const clickText = text => `Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()===${JSON.stringify(text)}).click()`;
const input = (selector,value) => `(() => { const el=document.querySelector(${JSON.stringify(selector)}); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(el,${JSON.stringify(value)}); el.dispatchEvent(new Event('input',{bubbles:true})); })()`;
async function fill(view, values) { for(const [selector,value] of Object.entries(values)) await evaluate(view,input(selector,value)); }
async function shot(view,vp,name) {
  const path = resolve(screenshotDir,`${vp}-${name}.png`);
  await Bun.write(path,await view.screenshot()); results.screenshots.push(path);
}
const modal = `document.querySelector('[role="dialog"][aria-label="Add Machine"]')`;
try {
 for(const vp of [1280,390]) {
  const view = new Bun.WebView({backend:'chrome',headless:true,width:vp,height:vp===1280?800:844});
  try {
   await view.navigate(harnessUrl);
   await view.cdp('Emulation.setDeviceMetricsOverride',{width:vp,height:vp===1280?800:844,deviceScaleFactor:1,mobile:false});
   await transition(view,`window.__qa && document.querySelectorAll('[data-machine-id]').length===4`);
   check(vp,'Default mixed inventory / no old top navigation',await evaluate(view,`document.querySelectorAll('[data-machine-id]').length===4 && !Array.from(document.querySelectorAll('button')).some(b=>['Machines','Access to This Machine','Connection Details'].includes(b.textContent.trim()))`));
   check(vp,'Single Add Machine with plus icon',await evaluate(view,`document.querySelectorAll('button[aria-label="Add Machine"]').length===1 && !!document.querySelector('button[aria-label="Add Machine"] svg')`));
   await shot(view,vp,'default');
   await transition(view,`document.querySelector('details').open`,`document.querySelector('details summary').click()`);
   check(vp,'Access disclosure opens real access component',await evaluate(view,`document.querySelector('details').open && document.querySelector('details').innerText.length>80`));
   await evaluate(view,`document.querySelector('details').scrollIntoView({block:'center'})`);
   await shot(view,vp,'access');
   await evaluate(view,`document.querySelector('details summary').click(); window.scrollTo(0,0); document.querySelector('button[aria-label="Add Machine"]').focus()`);
   await transition(view,modal,`document.querySelector('button[aria-label="Add Machine"]').click()`);
   check(vp,'Opening places focus inside modal',await evaluate(view,`${modal}.contains(document.activeElement)`));
   await transition(view,`document.querySelector('#ssh-auth-method')`,clickText('Connect with SSH'));
   check(vp,'Password selectable in basic form without Advanced',await evaluate(view,`!!document.querySelector('#ssh-auth-method option[value="password"]') && !document.querySelector('#ssh-identity')`));
   await transition(view,`document.querySelector('#ssh-password')`,`const select=document.querySelector('#ssh-auth-method'); select.value='password'; select.dispatchEvent(new Event('change',{bubbles:true}))`);
   await fill(view,{'#ssh-label':'Fixture Password Host','#ssh-hostname':'fail.fixture.invalid','#ssh-username':'fixture','#ssh-password':'fixture-secret-only'});
   check(vp,'Password masked and labeled',await evaluate(view,`document.querySelector('#ssh-password').type==='password' && !!document.querySelector('label[for="ssh-password"]')`));
   await shot(view,vp,'password-basic');
   await evaluate(view,`window.__qa.clearIpcCalls()`);
   await transition(view,`document.querySelector('[role="alert"]') && !document.querySelector('button[type="submit"]').disabled`,clickText('Connect SSH Machine'));
   let calls=await evaluate(view,`window.__qa.getIpcCalls()`);
   check(vp,'Failure: setter before probe, no save',calls.map(c=>c.cmd).join(',')==='cmd_ssh_set_password,cmd_ssh_test_connection',calls.map(c=>c.cmd));
   check(vp,'Failure retains fields/password',await evaluate(view,`document.querySelector('#ssh-password').value==='fixture-secret-only' && document.querySelector('#ssh-hostname').value==='fail.fixture.invalid'`));
   await shot(view,vp,'password-error');
   await transition(view,`!${modal}`,clickText('Cancel'));
   await transition(view,modal,`document.querySelector('button[aria-label="Add Machine"]').click()`);
   await transition(view,`document.querySelector('#ssh-auth-method')`,clickText('Connect with SSH'));
   await transition(view,`document.querySelector('#ssh-password')`,`const select=document.querySelector('#ssh-auth-method'); select.value='password'; select.dispatchEvent(new Event('change',{bubbles:true}))`);
   check(vp,'Cancel/reopen clears password and form',await evaluate(view,`document.querySelector('#ssh-password').value==='' && document.querySelector('#ssh-hostname').value===''`));
   await fill(view,{'#ssh-label':'Fixture Password Host','#ssh-hostname':'success.fixture.invalid','#ssh-username':'fixture','#ssh-password':'fixture-secret-only'});
   await evaluate(view,`window.__qa.clearIpcCalls()`);
   await transition(view,`!document.querySelector('#ssh-password') && Array.from(document.querySelectorAll('button')).some(b=>b.textContent.trim()==='Done')`,clickText('Connect SSH Machine'));
   calls=await evaluate(view,`window.__qa.getIpcCalls()`);
   const relevant=calls.filter(c=>['cmd_ssh_set_password','cmd_ssh_test_connection','cmd_ssh_update_host'].includes(c.cmd));
   check(vp,'Success: setter -> probe -> save',relevant.map(c=>c.cmd).join(',')==='cmd_ssh_set_password,cmd_ssh_test_connection,cmd_ssh_update_host',relevant.map(c=>c.cmd));
   check(vp,'Secret excluded from probe/save/persisted host',!JSON.stringify(relevant.slice(1)).includes('fixture-secret-only') && await evaluate(view,`!JSON.stringify(window.__qa.getPersistedHosts()).includes('fixture-secret-only')`));
   check(vp,'Success keeps keyboard focus within modal',await evaluate(view,`${modal}.contains(document.activeElement)`),await evaluate(view,`({tag:document.activeElement.tagName,machine:document.activeElement.getAttribute('data-machine-id')})`));
   await shot(view,vp,'success');
   await transition(view,`!${modal}`,clickText('Done'));
   await transition(view,`document.querySelector('button[aria-label="Edit Dev Server"]')`,`document.querySelector('button[aria-label="Details for Dev Server"]').click()`);
   await transition(view,`document.querySelector('#edit-ssh-password')`,`document.querySelector('button[aria-label="Edit Dev Server"]').click()`);
   check(vp,'Existing password machine Edit offers empty reauth field',await evaluate(view,`document.querySelector('#edit-ssh-password').value===''`));
   await fill(view,{'#edit-ssh-password':'fixture-reauth-only'});
   await evaluate(view,`document.querySelector('#edit-ssh-password').scrollIntoView({block:'center'})`);
   await shot(view,vp,'edit-reauth');
   await evaluate(view,`window.__qa.clearIpcCalls()`);
   await transition(view,`!document.querySelector('#edit-ssh-password')`,clickText('Save Changes'));
   calls=await evaluate(view,`window.__qa.getIpcCalls()`);
   check(vp,'Edit reauth setter -> probe -> save, secret excluded',calls.slice(0,3).map(c=>c.cmd).join(',')==='cmd_ssh_set_password,cmd_ssh_test_connection,cmd_ssh_update_host' && !JSON.stringify(calls.slice(1)).includes('fixture-reauth-only'),calls.map(c=>c.cmd));
   await evaluate(view,`window.__qa.resetEscapeCounts(); window.scrollTo(0,0); document.querySelector('button[aria-label="Add Machine"]').focus()`);
   await transition(view,modal,`document.querySelector('button[aria-label="Add Machine"]').click()`);
   await evaluate(view,`const els=${modal}.querySelectorAll('button:not([disabled]), input:not([disabled]),select:not([disabled]),textarea:not([disabled])'); els[els.length-1].focus()`);
   await view.press('Tab');
   check(vp,'Tab wraps to first modal control',await evaluate(view,`document.activeElement===${modal}.querySelector('button')`));
   // Subscribe to removal before actual keyboard dispatch via CDP.
   await evaluate(view,`window.__closed = new Promise((resolve,reject)=>{ const o=new MutationObserver(()=>{if(!${modal}){o.disconnect();clearTimeout(t);resolve(true)}});o.observe(document,{childList:true,subtree:true});const t=setTimeout(()=>{o.disconnect();reject(new Error('Escape timeout'))},8000) }); true`);
   await view.press('Escape'); await evaluate(view,`window.__closed`);
   check(vp,'Escape closes only child, restores trigger',await evaluate(view,`window.__qa.getParentEscapeCount()===0 && window.__qa.getWindowEscapeCount()===0 && document.activeElement.getAttribute('aria-label')==='Add Machine'`));
   check(vp,'No horizontal document overflow',await evaluate(view,`document.documentElement.scrollWidth<=${vp}`));
   await shot(view,vp,'final');
  } catch(error) { results.errors.push({vp,error:String(error)}); console.error(error); }
  finally { view.close(); }
 }
} finally {
 await vite.close();
 await Bun.write(resolve(outDir,'results.json'),JSON.stringify(results,null,2));
}
if(results.errors.length || results.checks.some(c=>!c.pass)) process.exitCode=1;
