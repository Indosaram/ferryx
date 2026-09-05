// Real isolated WebKit viewport, no desktop/daemon or user browser profile.
import assert from "node:assert/strict";
import { installDesignOverlay } from "./overlay.ts";
const view = new Bun.WebView({ width: 320, height: 240, persistent: false });
const deadline = setTimeout(() => { view.close(); console.error("WEBVIEW_PROOF_TIMEOUT"); process.exit(1); }, 15_000);
const identity = { browserId: "fixture", webviewLabel: "private", generation: "1", operationId: "proof", viewportRevision: 1 };
try {
  await view.navigate('data:text/html,<style>body{margin:0}button{position:absolute;left:20px;top:30px;width:120px;height:64px;background:rgb(20,100,200);border:0}</style><button id="sample" data-testid="design-element" onclick="window.clicks++">Select me</button><script>window.clicks=0;window.events=[]</script>');
  const evaluate = script => view.evaluate(`(()=>{${script}})()`);
  const arm = mode => evaluate(`(${installDesignOverlay.toString()})(${JSON.stringify(identity)},${JSON.stringify(mode)},e=>window.events.push(e)); return true`);
  await arm("element");
  await evaluate(`document.querySelector('button').dispatchEvent(new MouseEvent('pointermove',{bubbles:true,clientX:40,clientY:40})); document.querySelector('button').click(); return true`);
  const result = await view.evaluate(`({event:events[0],clicks,overlay:!!document.querySelector('[data-ferryx-design-overlay]')})`);
  assert.equal(result.event.type, "selected"); assert.deepEqual(result.event.selection.rect, { x:20,y:30,width:120,height:64 });
  assert.equal(result.event.selection.element.css["background-color"], "rgb(20, 100, 200)");
  assert.equal(result.clicks, 0); assert.equal(result.overlay, false);
  const png = new Uint8Array(await (await view.screenshot({ format: "png" })).arrayBuffer());
  assert.deepEqual([...png.slice(0,8)], [137,80,78,71,13,10,26,10]);
  if (process.env.DESIGN_VIEWPORT_PNG) await Bun.write(process.env.DESIGN_VIEWPORT_PNG, png);
  await evaluate("document.querySelector('button').click(); return true"); assert.equal(await view.evaluate("clicks"),1);
  await arm("rectangle");
  await evaluate(`window.dispatchEvent(new PointerEvent('pointerdown',{button:0,clientX:200,clientY:150}));window.dispatchEvent(new PointerEvent('pointerup',{button:0,clientX:10,clientY:20}));return true`);
  assert.deepEqual(await view.evaluate("events[1].selection.rect"), { x:10,y:20,width:190,height:130 });
  await arm("element"); await evaluate(`window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));return true`);
  assert.equal(await view.evaluate("events[2].type"),"cancelled");
  await arm("element"); await view.resize(400,300);
  assert.equal(await view.evaluate("events[3].type"),"invalidated");
  assert.equal(await view.evaluate("document.querySelectorAll('[data-ferryx-design-overlay]').length"),0);
  console.log(JSON.stringify({ status:"passed", boundary:"Bun.WebView WebKit injected actual page", assertions:12, pngBytes:png.length, nativeAppCapture:false }));
} finally { clearTimeout(deadline); view.close(); console.log(JSON.stringify({cleanup:"private WebView closed; data URL only; no listeners, ports, files, PTYs or daemon"})); }
