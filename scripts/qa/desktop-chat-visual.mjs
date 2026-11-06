import { mkdir } from "node:fs/promises";

const output = process.argv[2];
if (!output) throw new Error("Pass an isolated evidence directory");
await mkdir(output, { recursive: true });
const browser = Bun.spawn([
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "--headless=new", "--disable-gpu", "--no-first-run", "--remote-debugging-port=0",
  `--user-data-dir=${output}/cdp-profile`, "about:blank",
], { stdout: "pipe", stderr: "pipe" });
const deadline = setTimeout(() => browser.kill(), 60000);
try {
  let stderr = "";
  let address;
  for await (const chunk of browser.stderr) {
    stderr += new TextDecoder().decode(chunk);
    address = stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/)?.[1];
    if (address) break;
  }
  if (!address) throw new Error(`No DevTools endpoint: ${stderr}`);
  const socket = new WebSocket(address);
  const pending = new Map();
  let sequence = 0;
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data);
    if (message.id) {
      const request = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) request?.reject(new Error(JSON.stringify(message.error)));
      else request?.resolve(message.result);
    } else if (message.method === "Runtime.exceptionThrown") {
      console.error(JSON.stringify(message.params));
    }
  });
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  await send("Runtime.enable", {}, sessionId);
  await send("Page.enable", {}, sessionId);
  await send("Storage.clearDataForOrigin", {
    origin: "http://127.0.0.1:43917", storageTypes: "all",
  }, sessionId);
  await send("Page.addScriptToEvaluateOnNewDocument", {
    source: `document.addEventListener('DOMContentLoaded',()=>{const style=document.createElement('style');style.textContent='*,*::before,*::after{transition:none!important;animation:none!important}';document.head.append(style)})`,
  }, sessionId);
  await send("Page.navigate", { url: "http://127.0.0.1:43917/desktop-chat-qa.html" }, sessionId);
  const ready = await send("Runtime.evaluate", {
    expression: `new Promise((resolve,reject)=>{const ready=()=>document.body.innerText.includes('The pane keeps its terminal session');if(ready())return resolve(true);const o=new MutationObserver(()=>{if(ready()){o.disconnect();resolve(true)}});o.observe(document,{childList:true,subtree:true});setTimeout(()=>{o.disconnect();reject(new Error(document.body.innerText || 'Blank chat page'))},15000)})`,
    awaitPromise: true, returnByValue: true,
  }, sessionId);
  if (ready.exceptionDetails) throw new Error(JSON.stringify(ready.exceptionDetails));
  await send("Runtime.evaluate", { expression: "document.querySelector('textarea').focus()" }, sessionId);
  await send("Input.insertText", { text: "QA_SENT_MESSAGE" }, sessionId);
  const interaction = await send("Runtime.evaluate", {
    expression: `new Promise((resolve,reject)=>{const observer=new MutationObserver(check);function check(){const button=document.querySelector('button[aria-label="Send message"]');if(button&&!button.disabled){observer.disconnect();button.click();resolve(true)}}observer.observe(document,{subtree:true,attributes:true,childList:true});check();setTimeout(()=>{observer.disconnect();reject(new Error('Send did not enable'))},5000)})`,
    awaitPromise: true, returnByValue: true,
  }, sessionId);
  if (interaction.exceptionDetails) throw new Error(JSON.stringify(interaction.exceptionDetails));
  for (const [name, width, height, theme] of [
    ["desktop-dark", 1280, 800, "dark"], ["desktop-light", 1280, 800, "light"],
    ["narrow-dark", 390, 844, "dark"], ["narrow-light", 390, 844, "light"],
  ]) {
    await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }, sessionId);
    await send("Runtime.evaluate", { expression: `document.documentElement.classList.toggle('dark', ${theme === "dark"});document.documentElement.dataset.theme=${JSON.stringify(theme)}` }, sessionId);
    const { data } = await send("Page.captureScreenshot", { format: "png" }, sessionId);
    await Bun.write(`${output}/${name}.png`, Buffer.from(data, "base64"));
    console.log(`CAPTURED ${name}`);
  }
  await send("Browser.close");
  socket.close();
} finally {
  clearTimeout(deadline);
  browser.kill();
}
