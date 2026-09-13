import { createServer } from "../../../ui/node_modules/vite/dist/node/index.js";
import react from "../../../ui/node_modules/@vitejs/plugin-react/dist/index.js";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
process.chdir(resolve(import.meta.dir, "../../../ui"));
const phase = process.argv[2] ?? "baseline", root = resolve(import.meta.dir, "../../.."), out = resolve(import.meta.dir, phase);
await mkdir(out, { recursive: !0 });
const workspaceId = "qa-project-" + "very-long-project-name-".repeat(6), slug = "feature-" + "long-unbroken-worktree-label-".repeat(7), contexts = Array.from({ length: 28 }, (_, i) => ({ workspaceId, worktreeSlug: i ? `${slug}-${i}` : slug, worktreeLabel: i ? `${slug}-${i}` : slug, attention: i % 3 === 0 ? "working" : "done" })), tabs = [{ id: "tab-original", label: "Shell", sessionId: "pty-original" }];
let state;
const reset = (empty = !1) => {
  state = { activeContext: { workspaceId, worktreeSlug: slug, worktreeLabel: slug, activeTabId: empty ? null : "tab-original", activeTerminal: empty ? null : { sessionId: "pty-original", running: !0 }, terminalTabs: empty ? [] : [...tabs] }, contexts };
};
reset();
const requests = [], messages = [], events = new EventTarget, sockets = new Set;
let frameSerial = 0;
function frame(ws, marker = "READY") {
  const { cols, rows, target } = ws.data, lines = Array.from({ length: Math.min(rows, 18) }, (_, index) => {
    const text = index === 0 ? `${marker} ${target.split("/").pop()}` : index === 17 ? "$ " : `output ${String(index).padStart(2, "0")} - browser grid fixture`;
    return { index, runs: [{ text: text.slice(0, cols), cells: Math.min(text.length, cols), fg: null, bg: null, attrs: 0 }] };
  });
  ws.send(JSON.stringify({ type: "grid", cols, rows, cursor: { x: 2, y: Math.min(17, rows - 1), visible: !0, blinking: !1, wideTail: !1, visualStyle: "bar" }, lines }));
}
function signal(name, predicate) {
  return new Promise((resolve, reject) => {
    const listener = (event) => {
      if (predicate(event.detail)) {
        clearTimeout(timeout);
        events.removeEventListener(name, listener);
        resolve(event.detail);
      }
    }, timeout = setTimeout(() => {
      events.removeEventListener(name, listener);
      reject(Error(`No ${name} signal`));
    }, 8000);
    events.addEventListener(name, listener);
  });
}
const vite = await createServer({ configFile: !1, root, plugins: [react()], css: { postcss: resolve(root, "ui") }, resolve: { alias: { "@": resolve(root, "ui/src"), react: resolve(root, "ui/node_modules/react"), "react-dom": resolve(root, "ui/node_modules/react-dom") } }, server: { host: "127.0.0.1", port: 0, hmr: !1, watch: { ignored: ["**/src-tauri/**", "**/target/**", "**/docs/evidence/**"] } } });
await vite.listen();
const viteOrigin = vite.resolvedUrls.local[0], server = Bun.serve({
  hostname: "127.0.0.1",
  port: 0,
  async fetch(req, server) {
    const url = new URL(req.url);
    if (req.headers.get("upgrade") === "websocket") {
      if (server.upgrade(req, { data: { target: url.pathname, cols: Number(url.searchParams.get("cols")) || 40, rows: Number(url.searchParams.get("rows")) || 30 } }))
        return;
      return new Response("Upgrade failed", { status: 400 });
    }
    if (url.pathname.startsWith("/api/")) {
      const body = req.method === "POST" ? await req.json() : null, item = { method: req.method, path: url.pathname, body, authorization: req.headers.get("authorization") };
      requests.push(item);
      events.dispatchEvent(new CustomEvent("request", { detail: item }));
      if (url.pathname === "/api/v1/socket-ticket")
        return Response.json({ ticket: "qa-target-bound-ticket" });
      if (url.pathname === "/api/v1/workspace/state")
        return Response.json(state);
      if (url.pathname === "/api/v1/workspace/select") {
        state.activeContext = { ...state.activeContext, activeTabId: body.tabId ?? state.activeContext.activeTabId };
        if (body.createTerminal) {
          const tab = { id: "tab-created", label: "Terminal 2", sessionId: "pty-created" };
          state.activeContext = { ...state.activeContext, activeTabId: tab.id, activeTerminal: { sessionId: tab.sessionId, running: !0 }, terminalTabs: [...state.activeContext.terminalTabs, tab] };
        }
        for (const ws of sockets)
          if (ws.data.target.endsWith("/events"))
            ws.send(JSON.stringify({ event: "remote_active_selection_changed", payload: state.activeContext }));
        return Response.json({ ok: !0 });
      }
      return Response.json({ error: "Unexpected QA request", item }, { status: 500 });
    }
    return fetch(new URL(url.pathname + url.search, viteOrigin));
  },
  websocket: {
    open(ws) {
      sockets.add(ws);
      if (ws.data.target.includes("/terminal/"))
        frame(ws);
      events.dispatchEvent(new CustomEvent("open", { detail: ws.data }));
    },
    close(ws) {
      sockets.delete(ws);
    },
    message(ws, raw) {
      const item = typeof raw === "string" ? { target: ws.data.target, ...JSON.parse(raw) } : { target: ws.data.target, type: "input", text: new TextDecoder().decode(raw) };
      messages.push(item);
      events.dispatchEvent(new CustomEvent("message", { detail: item }));
      if (item.type === "resize") {
        ws.data.cols = item.cols;
        ws.data.rows = item.rows;
        frame(ws, `RESIZED${++frameSerial}`);
      }
    }
  }
}), results = [], evaluate = (view, code) => view.evaluate(`eval(${JSON.stringify(code)})`), check = (name, pass, detail) => results.push({ name, pass, detail });
try {
  for (const width of [360, 390, 768, 1280]) {
    reset();
    const view = new Bun.WebView({ backend: "chrome", headless: !0, width, height: 780 });
    try {
      await view.navigate("about:blank");
      await view.cdp("Emulation.setDeviceMetricsOverride", { width, height: 780, deviceScaleFactor: 1, mobile: width < 768 });
      await view.cdp("Emulation.setTouchEmulationEnabled", { enabled: width < 768 });
      await view.navigate(`http://127.0.0.1:${server.port}/docs/evidence/mobile-remote-20260913/index.html`);
      await evaluate(view, `new Promise((resolve,reject)=>{const check=()=>{if(document.querySelector('[data-grid-line="0"]')){o.disconnect();clearTimeout(t);resolve(true)}};const o=new MutationObserver(check);const t=setTimeout(()=>{o.disconnect();reject(new Error(document.body.innerText))},10000);o.observe(document,{subtree:true,childList:true});check()})`);
      await evaluate(view, `document.querySelector('[aria-label="Change workspace context"]').click(); qaWait('[role="dialog"]')`);
      const bounds = JSON.parse(await evaluate(view, `JSON.stringify((() => { const dialog = document.querySelector('[role="dialog"]'); const list = dialog.lastElementChild; const doc = document.documentElement; list.scrollLeft = 100; list.scrollTop = 150; return { viewport: innerWidth, documentWidth: doc.scrollWidth, dialogWidth: dialog.clientWidth, dialogScroll: dialog.scrollWidth, listWidth: list.clientWidth, listScrollWidth: list.scrollWidth, horizontalScroll: list.scrollLeft, verticalScroll: list.scrollTop, listHeight: list.clientHeight, listScrollHeight: list.scrollHeight }; })())`));
      check(`${width}: list horizontal containment`, bounds.documentWidth <= width && bounds.listScrollWidth <= bounds.listWidth && bounds.horizontalScroll === 0, bounds);
      check(`${width}: list vertical scroll`, bounds.verticalScroll > 0, bounds);
      await Bun.write(`${out}/${width}-worktrees.png`, await view.screenshot());
      await evaluate(view, `document.querySelector('[aria-label="Close workspace context"]').click();`);
      await evaluate(view, `document.querySelector('[data-testid="remote-terminal-input-sink"]').blur(); window.qaFocusEvents=[]; document.addEventListener('focusin', e=>qaFocusEvents.push(e.target.getAttribute('data-testid')||e.target.tagName));`);
      const scrolled = signal("message", (item) => item.type === "scroll"), touch = await evaluate(view, `(() => { const el = document.querySelector('[data-testid="remote-terminal-grid"]'); const sink=document.querySelector('textarea'); const rect=el.getBoundingClientRect(); const x=rect.x+40, y=rect.bottom-20; el.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'touch',clientX:x,clientY:y})); const t=(yy)=>new Touch({identifier:1,target:el,clientX:x,clientY:yy}); el.dispatchEvent(new TouchEvent('touchstart',{bubbles:true,touches:[t(y)],changedTouches:[t(y)]})); el.dispatchEvent(new TouchEvent('touchmove',{bubbles:true,touches:[t(y-120)],changedTouches:[t(y-120)]})); el.dispatchEvent(new TouchEvent('touchend',{bubbles:true,touches:[],changedTouches:[t(y-120)]})); return JSON.stringify({focused:document.activeElement===sink,focusEvents:qaFocusEvents}); })()`), scroll = await scrolled;
      check(`${width}: touch scroll does not focus sink`, !JSON.parse(touch).focused && !JSON.parse(touch).focusEvents.includes("remote-terminal-input-sink"), { ...JSON.parse(touch), scroll });
      const point = JSON.parse(await evaluate(view, `JSON.stringify((()=>{const r=document.querySelector('[data-testid="remote-terminal-grid"]').getBoundingClientRect();return {x:r.x+30,y:r.y+50}})())`));
      await view.click(point.x, point.y);
      check(`${width}: real pointer click focuses sink`, await evaluate(view, "document.activeElement===document.querySelector('textarea')"), await evaluate(view, "document.activeElement.outerHTML.slice(0,180)"));
      await evaluate(view, "document.querySelector('textarea').blur(); window.qaTrusted=[]; document.addEventListener('touchend', e=>qaTrusted.push(e.isTrusted),{once:true});");
      await view.cdp("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: point.x, y: point.y }] });
      await view.cdp("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
      check(`${width}: trusted touch tap focuses sink`, await evaluate(view, "document.activeElement===document.querySelector('textarea') && qaTrusted[0]===true"), await evaluate(view, "JSON.stringify(qaTrusted)"));
      const gridReady = evaluate(view, `qaWait('[data-grid-line="0"]','UPDATED')`);
      for (const ws of sockets)
        if (ws.data.target.includes("/terminal/"))
          frame(ws, "UPDATED");
      await gridReady;
      check(`${width}: focus survives grid update`, await evaluate(view, "document.activeElement===document.querySelector('textarea')"), {});
      const resize = signal("message", (item) => item.type === "resize");
      await view.cdp("Emulation.setDeviceMetricsOverride", { width, height: 600, deviceScaleFactor: 1, mobile: width < 768 });
      await resize;
      check(`${width}: focus survives viewport resize`, await evaluate(view, "document.activeElement===document.querySelector('textarea')"), {});
      const restored = signal("message", (item) => item.type === "resize");
      await view.cdp("Emulation.setDeviceMetricsOverride", { width, height: 780, deviceScaleFactor: 1, mobile: width < 768 });
      await restored;
      await Bun.write(`${out}/${width}-terminal.png`, await view.screenshot());
      const start = messages.length, barrier = signal("message", (item) => item.type === "input" && item.text === "!");
      await evaluate(view, "(() => { const s=document.querySelector('textarea'); s.focus(); s.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''})); s.value='\uD55C'; s.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:'\uD55C'})); s.dispatchEvent(new InputEvent('input',{bubbles:true,data:'\uD55C',inputType:'insertCompositionText',isComposing:true})); s.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'\uD55C'})); s.value='\uD55C'; s.dispatchEvent(new InputEvent('input',{bubbles:true,data:'\uD55C',inputType:'insertText'})); s.value='!'; s.dispatchEvent(new InputEvent('input',{bubbles:true,data:'!',inputType:'insertText'})); })()");
      await barrier;
      const inputs = messages.slice(start).filter((item) => item.type === "input").map((item) => item.text);
      check(`${width}: Hangul trailing input commits once`, inputs.join("") === "\uD55C!", inputs);
      for (const ordering of ['input-before-end', 'cancel', 'blur']) {
        const before = messages.length;
        const done = signal('message', item => item.type === 'input' && item.text === '#');
        await evaluate(view, `(() => { const s=document.querySelector('textarea');s.focus();s.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));s.value='한';s.dispatchEvent(new InputEvent('input',{bubbles:true,data:'한',inputType:'insertCompositionText',isComposing:true})); if(${JSON.stringify(ordering)}==='blur')s.blur();else s.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:${ordering === 'cancel' ? "''" : "'한'"}}));s.value='#';s.dispatchEvent(new InputEvent('input',{bubbles:true,data:'#',inputType:'insertText'}));})()`);
        await done;
        const text = messages.slice(before).filter(item => item.type === 'input').map(item => item.text).join('');
        check(`${width}: Hangul ${ordering}`, text === (ordering === 'input-before-end' ? '한#' : '#'), text);
      }
      const newButton = await evaluate(view, "Array.from(document.querySelectorAll('button')).find(b=>/new terminal|new tab|add terminal/i.test(b.getAttribute('aria-label')||b.textContent))?.getAttribute('aria-label')??null");
      check(`${width}: new terminal action exists`, !!newButton, newButton);
      if (phase === "post-fix" && newButton) {
        const createdRequest = signal("request", (item) => item.body?.createTerminal === true);
        await evaluate(view, `(() => {const ready=qaWait('[data-grid-line="0"]','pty-created');document.querySelector('[aria-label="New terminal tab"]').click();return ready;})()`);
        const request = await createdRequest;
        check(`${width}: new tab target and resulting terminal`, request.path === "/api/v1/workspace/select" && request.body.workspaceId === workspaceId && request.body.worktreeSlug === slug && !request.body.tabId && !request.body.sessionId && await evaluate(view, `document.querySelector('[role="tab"][aria-selected="true"]')?.textContent.includes('Terminal 2')`), request);
        await Bun.write(`${out}/${width}-created-terminal.png`, await view.screenshot());
      }
      const controls = JSON.parse(await evaluate(view, `JSON.stringify(Array.from(document.querySelectorAll('header button,[aria-label="New terminal tab"],[aria-label="Previous terminal tab"],[aria-label="Next terminal tab"]')).map(el=>{const r=el.getBoundingClientRect(); return {label:el.getAttribute('aria-label')||el.textContent,left:r.left,right:r.right,top:r.top,bottom:r.bottom,width:r.width};}))`));
      check(`${width}: controls not cropped`, controls.every((r) => r.left >= 0 && r.right <= width && r.top >= 0 && r.bottom <= 780 && r.width > 0), controls);
      await Bun.write(`${out}/${width}-controls.png`, Buffer.from((await view.cdp("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width, height: 110, scale: 1 } })).data, "base64"));
    } finally {
      view.close();
    }
    if (phase === "post-fix") {
      reset(!0);
      const empty = new Bun.WebView({ backend: "chrome", headless: !0, width, height: 780 });
      try {
        await empty.navigate("about:blank");
        await empty.cdp("Emulation.setDeviceMetricsOverride", { width, height: 780, deviceScaleFactor: 1, mobile: width < 768 });
        await empty.navigate(`http://127.0.0.1:${server.port}/docs/evidence/mobile-remote-20260913/index.html`);
        await evaluate(empty, `new Promise((resolve,reject)=>{const check=()=>{const b=document.querySelector('[aria-label="New terminal tab"]');if(b&&!b.disabled){o.disconnect();clearTimeout(t);resolve(true)}};const o=new MutationObserver(check);const t=setTimeout(()=>{o.disconnect();reject(new Error('No empty-context create action'))},8000);o.observe(document,{subtree:true,childList:true,attributes:true});check()})`);
        await Bun.write(`${out}/${width}-empty-terminal.png`, await empty.screenshot());
        const requestSignal = signal("request", (item) => item.body?.createTerminal === true);
        await evaluate(empty, `(() => {const ready=qaWait('[data-grid-line="0"]','pty-created');document.querySelector('[aria-label="New terminal tab"]').click();return ready;})()`);
        const request = await requestSignal;
        check(`${width}: zero-terminal context creates selected terminal`, request.body.workspaceId === workspaceId && request.body.worktreeSlug === slug && await evaluate(empty, `document.querySelector('[role="tab"][aria-selected="true"]')?.textContent.includes('Terminal 2')`), request);
        await Bun.write(`${out}/${width}-empty-created.png`, await empty.screenshot());
      } finally {
        empty.close();
      }
    }
  }
} finally {
  await Bun.write(`${out}/results.json`, JSON.stringify({ phase, browser: "Bun.WebView headless Chrome (default WebKit lacks TouchEvent)", bun: Bun.version, results, requests, messages, limits: "Browser integration with real remote components and mocked HTTP/WebSocket boundary; synthetic touch/composition events are not native phone IME, keyboard or live PTY verification." }, null, 2));
  server.stop(!0);
  await vite.close();
}
console.log(JSON.stringify(results, null, 2));
if (results.some(result => !result.pass)) process.exitCode = 1;
