import { createRequire } from "node:module";
import { mkdirSync, readFileSync, copyFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID, createHash } from "node:crypto";
import { assert, deadline, repoRoot, sha256, setupIsolatedProfile, spawnRustGatewayFixture,
  validateConsumerReceipt, validateAndroidDeviceEvidence, validateSpeechEvidence, writeReport } from "./herdr-mobile-fixtures.mjs";

function parseArgs() {
  const args = {};
  const allowed = new Set(["evidence-dir", "gateway-binary", "ui-dist", "candidate-manifest", "consumer-receipt", "android-device-evidence", "native-speech-evidence", "browser-channel", "allow-host"]);
  for (let i=2; i<process.argv.length; i+=2) {
    const key = process.argv[i].replace(/^--/, "");
    assert(allowed.has(key) && process.argv[i+1], "Unknown or incomplete option: " + process.argv[i]);
    args[key] = process.argv[i+1];
  }
  for (const key of ["evidence-dir", "gateway-binary", "ui-dist", "candidate-manifest"]) assert(args[key], "Missing --" + key);
  return args;
}
function signal() { let resolveSignal; const promise = new Promise(r => {resolveSignal=r;}); return {promise, resolve:resolveSignal}; }
async function scoped(response, requestId) {
  const body = await response.json();
  assert(typeof body.ok === "boolean" && body.requestId === requestId, "Not a single correlated ScopeResult envelope");
  assert(!body.data?.ok, "Nested response envelope");
  if (body.ok) assert(response.ok(), "Success envelope with HTTP failure");
  else assert(!response.ok() && typeof body.error?.code === "string", "Invalid scoped error");
  return body;
}
async function observeValue(locator, value) {
  await locator.page().waitForFunction(({selector,value}) => document.querySelector(selector)?.value === value,
    {selector:'[data-testid="chat-composer-textarea"]',value}, {timeout:15000});
}
// A Playwright waiter created before a potentially-hanging action can reject at its own
// timeout while nothing awaits it; Node escalates that to an unhandled rejection and kills
// the process before report.json is written, losing every scenario result. Attaching a
// no-op catch marks the promise handled; a later `await` on the same promise still throws
// the identical error, so the timeout surfaces as a scenario FAIL instead of a crash.
function handled(waiter) { waiter.catch(() => {}); return waiter; }

export async function runMobileAcceptance() {
  const args = parseArgs();
  assert(process.platform === "win32" && args["allow-host"] === "true", "Acceptance requires Windows and explicit --allow-host true authorization");
  const candidate = JSON.parse(readFileSync(args["candidate-manifest"], "utf8"));
  assert(candidate.candidateId && candidate.sourceManifestSha256 && candidate.fixtureSha256 && candidate.uiFiles, "Candidate build provenance is incomplete");
  assert(sha256(args["gateway-binary"]) === candidate.fixtureSha256, "Fixture binary differs from candidate");
  const requiredTestIds = ["remote-terminal-grid","remote-terminal-line-input","remote-terminal-input-mode-toggle",
    "remote-terminal-preedit","remote-terminal-input-sink","remote-view-mode-terminal","remote-view-mode-chat",
    "chat-composer-textarea","send-button","attach-file-button","file-upload-input","chat-composer-attachments-staging"];
  const shippedScripts = [];
  for (const [file, hash] of Object.entries(candidate.uiFiles)) {
    const path = resolve(args["ui-dist"], file);
    assert(path.startsWith(resolve(args["ui-dist"]) + "\\") || path.startsWith(resolve(args["ui-dist"]) + "/"), "UI manifest path escapes dist");
    assert(sha256(path) === hash, "UI candidate hash mismatch: " + file);
    if (/\.(?:m?js)$/.test(file)) shippedScripts.push(readFileSync(path,"utf8"));
  }
  assert(candidate.uiFiles["index.html"], "Candidate must include real built React index.html");
  for (const id of requiredTestIds) assert(shippedScripts.some(source=>source.includes(id)), "Hashed candidate UI lacks required test ID: " + id);
  mkdirSync(args["evidence-dir"], {recursive:true});
  const report = {schema:1, candidateId:candidate.candidateId, sourceManifestSha256:candidate.sourceManifestSha256,
    candidateManifestSha256:sha256(args["candidate-manifest"]), requiredTestIds, scenarios:{}, errors:[], cleanup:null,
    androidNativeGate:validateAndroidDeviceEvidence(args["android-device-evidence"],candidate),
    iosNativeGate:{status:"UNMET_PREREQUISITE",reason:"No native iOS receipt supplied"},
    nativeSpeechGate:validateSpeechEvidence(args["native-speech-evidence"],candidate),
    remoteHistoryGate:{status:"UNMET_PREREQUISITE",reason:"This fixture proves local owning-daemon history; paired-host transport needs its own receipt"},
    overallStatus:"BLOCKED"};
  let fixture, browser, profile;
  try {
    profile = setupIsolatedProfile();
    const bin = join(profile.root,"bin"); mkdirSync(bin);
    copyFileSync(args["gateway-binary"],join(bin,"codex.exe"));
    for (const key of Object.keys(profile.env)) if (key.toLowerCase()==="path") delete profile.env[key];
    profile.env.PATH = bin + ";" + (process.env.PATH ?? process.env.Path ?? "");
    profile.env.FERRYX_UI_DIST_DIR = resolve(args["ui-dist"]);
    fixture = await spawnRustGatewayFixture({fixtureBinary:resolve(args["gateway-binary"]),profile,allowHost:args["allow-host"] === "true"});
    const ready = fixture.readyInfo;
    const require = createRequire(join(repoRoot,"ui","package.json"));
    const {chromium} = require(process.env.PLAYWRIGHT_CORE_PATH || "playwright-core");
    browser = await chromium.launch({headless:true,channel:args["browser-channel"] === "chromium" ? undefined : args["browser-channel"] || "chrome"});
    for (const viewport of [{width:390,height:844},{width:360,height:800}]) {
      const context = await browser.newContext({viewport,isMobile:true,hasTouch:true});
      const page = await context.newPage();
      page.setDefaultTimeout(15000);
      page.on("pageerror", error => report.errors.push(error.message));
      const browserEvidence={console:[],requests:[],responses:[],events:[],rendered:{},
        fixtureTargets:ready.targets,capabilities:ready.capabilities,
        fixtureWorkspace:ready.workspaceState,sessionInventory:ready.sessionInventory};
      page.on("pageerror",error=>browserEvidence.console.push({type:"pageerror",text:error.message}));
      report.browserEvidence ??= {};
      report.browserEvidence[viewport.width]=browserEvidence;
      const pendingEvidence=new Set();
      const eventReady=signal(), callbackFrame=signal();
      page.on("console",message=>browserEvidence.console.push({type:message.type(),text:message.text()}));
      page.on("request",request=>{
        const path=new URL(request.url()).pathname;
        if(path.startsWith("/api/v1/")) browserEvidence.requests.push({path,method:request.method(),body:request.postData()});
      });
      page.on("response",response=>{
        const path=new URL(response.url()).pathname;
        if(!/^\/api\/v1\/(workspace\/state|sessions|capabilities|chat\/)/.test(path))return;
        const capture=response.text().then(body=>browserEvidence.responses.push({path,status:response.status(),body}),
          error=>browserEvidence.responses.push({path,status:response.status(),error:error.message}));
        pendingEvidence.add(capture); capture.finally(()=>pendingEvidence.delete(capture));
      });
      page.on("websocket",socket=>{
        if(new URL(socket.url()).pathname!=="/api/v1/events")return;
        socket.on("framereceived",({payload})=>{
          const text=typeof payload==="string"?payload:payload.toString("utf8");
          browserEvidence.events.push(text);
          try {const frame=JSON.parse(text);
            if(frame.type==="inventoryInvalidated")eventReady.resolve(frame);
            if(frame.type==="callback" && frame.callback?.text==="HERDR_CONTROLLED_APPROVAL")callbackFrame.resolve(frame);
          } catch {}
        });
      });
      await context.addInitScript(({token,deviceId,hostId,base}) => {
        localStorage.setItem("ferryx_remote_hosts",JSON.stringify({hosts:{[hostId]:{
          hostId,name:"Herdr isolated",address:base,transport:"mdns",authStatus:"paired",online:true,deviceToken:token,machineId:hostId
        }},activeHostId:hostId}));
        localStorage.setItem("ferryx_device_id_"+hostId,deviceId);
      },{token:ready.token,deviceId:ready.deviceId,hostId:ready.targets[0].hostId,base:ready.gatewayUrl});
      const api = context.request;
      const headers = {Authorization:"Bearer "+ready.token};
      async function post(path, data) { return api.post(ready.gatewayUrl+path,{headers,data}); }
      // Reset managed providers between viewports. Each viewport starts a real managed child via the UI's
      // Start button; without stopping it, the NEXT viewport's Start hits a correct 409
      // REQUEST_CONFLICT ("Stop the existing managed provider before starting another") because the
      // backendSessionId still holds a provider. That turned a product-correct response into a
      // viewport-dependent scenario failure. Stopping both fixture targets makes every viewport begin
      // from the same state; a stop for a session with no provider is a no-op.
      for (const resetTarget of ready.targets) {
        try { await post("/api/v1/chat/stop",{requestId:randomUUID(),target:resetTarget}); } catch {}
      }
      async function callbacks(target) {
        const response = await api.get(ready.gatewayUrl+"/api/v1/chat/callbacks?backendSessionId="+target.backendSessionId,{headers});
        const body=await response.json(); assert(response.ok() && body.ok && Array.isArray(body.data),"Callback discovery failed"); return body.data;
      }
      async function scenario(name, action) {
        const key=viewport.width+"_"+name;
        try { const evidence=await action(); report.scenarios[key]={status:"PASS",evidence}; }
        catch(error) { report.scenarios[key]={status:"FAIL",reason:error.message}; }
        browserEvidence.rendered[key]=await page.evaluate(()=>({
          bodyText:document.body.innerText,
          composerPresent:Boolean(document.querySelector('[data-testid="chat-composer-textarea"]')),
          callbacksText:document.querySelector('[data-testid="live-callbacks-container"]')?.textContent ?? null,
        }));
        await page.screenshot({path:join(args["evidence-dir"],key+".png"),fullPage:true});
      }
      try {
        await fixture.command({type:"focus",index:0},"focusChanged");
        const workspaceReady=handled(page.waitForResponse(r=>new URL(r.url()).pathname==="/api/v1/workspace/state" && r.ok()));
        await page.goto(ready.gatewayUrl,{waitUntil:"domcontentloaded"});
        browserEvidence.workspace=await (await workspaceReady).json();
        // A remote client cannot mirror-select a machine-owned session: `set_active_selection`
        // (remote/state.rs) clears on EITHER `machine_only(session_id)` OR a non-mirror-exposed
        // workspace, and a session created through the remote route is journal-owned. So the product,
        // not the fixture, decides which session is active. Assert against the session the served
        // workspace state actually declares active instead of assuming a particular fixture index.
        const servedSessionId=browserEvidence.workspace?.activeContext?.sessionId ?? null;
        const activeTarget=ready.targets.find(t=>t.backendSessionId===servedSessionId) ?? null;
        const otherTarget=ready.targets.find(t=>t.backendSessionId!==servedSessionId) ?? null;
        // The fixture's control commands are INDEX-based, so every one of them must address the session
        // the product actually made active - not a fixed index. Addressing index 0 while the active
        // session is a different target installs the fixture provider on the wrong session, which then
        // produces no callback and rewrites the wrong history.
        const activeIndex=ready.targets.findIndex(t=>t.backendSessionId===servedSessionId);
        const otherIndex=ready.targets.findIndex(t=>t.backendSessionId!==servedSessionId);
        assert(activeTarget,"the served workspace state must declare one of the fixture sessions active: "
          +JSON.stringify({servedSessionId,targets:ready.targets.map(t=>t.backendSessionId)}));
        await deadline(eventReady.promise,"real events inventory received");
        await page.getByTestId("remote-view-mode-chat").click();
        const composer=page.getByTestId("chat-composer-textarea");
        await composer.waitFor({state:"visible"});
        await scenario("production_launch",async()=>{
          const launchResponse=handled(page.waitForResponse(r=>new URL(r.url()).pathname==="/api/v1/chat/start"));
          await page.getByRole("button",{name:"Start Codex",exact:true}).click();
          const launch=await launchResponse;
          const uiTarget=launch.request().postDataJSON().target;
          browserEvidence.uiTarget=uiTarget;
          const launchText=await launch.text();
          browserEvidence.launch={status:launch.status(),body:launchText};
          const target=activeTarget;
          assert(["hostId","ownerId","epoch","backendSessionId"].every(key=>uiTarget[key]===target[key]),
            "UI TargetRef differs from the session the served workspace state declares active: "+JSON.stringify({uiTarget,target}));
          assert(launch.ok(),"Real UI launch failed: "+launchText);
          ready.threads[0]=JSON.parse(launchText).data.threadId;
          const sendResponse=handled(page.waitForResponse(r=>new URL(r.url()).pathname==="/api/v1/chat/send"));
          await composer.fill("HERDR_LAUNCH_TURN");
          await page.getByTestId("send-button").click();
          const response=await sendResponse;
          const requestId=response.request().postDataJSON().requestId;
          const sent=await scoped(response,requestId);
          assert(sent.ok && sent.data.requestId===requestId && sent.data.stage==="accepted","Production supervisor did not accept turn");
          const frame=await deadline(callbackFrame.promise,"production callback on real events socket");
          assert(frame.sessionId===target.backendSessionId,"Callback event session differs from selected fixture session");
          await page.getByText("HERDR_CONTROLLED_APPROVAL",{exact:true}).waitFor({state:"visible"});
          browserEvidence.callbackRendered=true;
          const live=await callbacks(target); const callback=live.find(c=>c.threadId===ready.threads[0]);
          assert(callback,"Controlled JSON-RPC callback was not registered/discovered");
          const replyId=randomUUID(); const replied=await scoped(await post("/api/v1/chat/reply",{...callback,requestId:replyId,target,result:{decision:"accept"}}),replyId);
          assert(replied.ok && replied.data.resolved,"Production child callback reply failed");
          const stopId=randomUUID(); assert((await scoped(await post("/api/v1/chat/stop",{requestId:stopId,target}),stopId)).ok,"Production child stop failed");
          const afterId=randomUUID(); const after=await scoped(await post("/api/v1/chat/send",{requestId:afterId,target,draft:{text:"MUST_NOT_SEND",attachments:[]}}),afterId);
          assert(!after.ok,"Stopped production provider remained usable");
          return {threadId:callback.threadId,callbackId:callback.callbackId,stopObserved:true};
        });
        await fixture.command({type:"useFixture",index:activeIndex},"fixtureProviderInstalled");
        await scenario("1_receipt_and_explicit_retry",async()=>{
          await composer.waitFor();
          await fixture.command({type:"rejectNext"},"rejectNextConfigured");
          await composer.fill("HERDR_HELD_DRAFT");
          const failed=handled(page.waitForResponse(r=>r.url().endsWith("/api/v1/chat/send")));
          await page.getByTestId("send-button").click(); await failed;
          await page.getByTestId("mobile-chat-held").waitFor();
          assert(await composer.inputValue()==="HERDR_HELD_DRAFT","Rejected draft lost");
          let sendCount=0; const count=req=>{if(req.url().endsWith("/api/v1/chat/send"))sendCount++;}; page.on("request",count);
          const discovered=handled(page.waitForResponse(r=>r.url().includes("/api/v1/chat/callbacks")));
          await page.reload(); await discovered;
          assert(sendCount===0,"Held draft replayed on reconnect");
          const retry=handled(page.waitForResponse(r=>r.url().endsWith("/api/v1/chat/send")));
          await page.getByTestId("mobile-chat-retry").click(); const response=await retry;
          const request=response.request().postDataJSON(); const body=await response.json();
          assert(body.ok && body.data.requestId===request.requestId && ["hostId","ownerId","epoch","backendSessionId"].every(k=>body.data.target[k]===request.target[k]),"Receipt correlation mismatch");
          await observeValue(composer,""); page.off("request",count);
          assert(sendCount===1,"Explicit retry dispatched more than once");
          return {requestId:request.requestId,stage:body.data.stage,sendCount};
        });
        await scenario("2_callback_freshness",async()=>{
          const target=activeTarget, suffix=randomUUID();
          await fixture.command({type:"publishCallback",index:activeIndex,id:"old-"+suffix,threadId:"fixture-thread",turnId:"old",text:"HERDR_OLD_CALLBACK"},"callbackPublished");
          const old=(await callbacks(target)).find(c=>c.callbackId==="old-"+suffix); assert(old,"Old callback absent");
          await fixture.command({type:"publishCallback",index:activeIndex,id:"new-"+suffix,threadId:"fixture-thread",turnId:"new",text:"HERDR_NEW_CALLBACK"},"callbackPublished");
          const requestId=randomUUID(); const stale=await scoped(await post("/api/v1/chat/reply",{...old,target,requestId,result:{decision:"accept"}}),requestId);
          assert(!stale.ok,"Stale callback was accepted");
          assert((await callbacks(target)).some(c=>c.callbackId==="new-"+suffix),"Stale reply consumed replacement callback");
          await page.reload(); await page.getByTestId("approval-accept").last().waitFor();
          const responsePromise=handled(page.waitForResponse(r=>r.url().endsWith("/api/v1/chat/reply")));
          await page.getByTestId("approval-accept").last().click(); const response=await responsePromise;
          assert((await response.json()).data?.resolved===true,"Live UI approval did not resolve");
          return {staleCode:stale.error.code};
        });
        await scenario("3_delayed_history_target_isolation",async()=>{
          const oldMarker="HERDR_OLD_"+randomUUID(), freshMarker="HERDR_FRESH_"+randomUUID();
          await fixture.command({type:"rewriteHistory",index:activeIndex,marker:oldMarker},"historyRewritten");
          await fixture.command({type:"rewriteHistory",index:otherIndex,marker:freshMarker},"historyRewritten");
          const captured=signal(), release=signal(), settled=signal(); let oldBody;
          const oldPath="**/api/v1/agent-history/"+activeTarget.backendSessionId+"*";
          await page.route(oldPath,async route=>{
            try { const response=await route.fetch(); oldBody=await response.json();
              assert(response.ok() && oldBody.items.some(item=>item.text===oldMarker),"Real old transcript not served");
              captured.resolve(); await deadline(release.promise,"release old history");
              await route.fulfill({response}); settled.resolve();
            } catch(error) { captured.resolve(error); settled.resolve(error); }
          },{times:1});
          try {
            await page.reload({waitUntil:"domcontentloaded"});
            const error=await deadline(captured.promise,"old history captured"); if(error)throw error;
            await fixture.command({type:"focus",index:otherIndex},"focusChanged");
            await page.getByText(freshMarker,{exact:true}).waitFor();
            release.resolve(); const delivery=await deadline(settled.promise,"old history delivery");
            if(delivery && !/closed|abort|cancel/i.test(delivery.message))throw delivery;
            const currentPath="/api/v1/agent-history/"+otherTarget.backendSessionId;
            const currentResponse=await page.waitForResponse(r=>r.url().includes(currentPath));
            await currentResponse.finished();
            await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>resolve())));
            assert(await page.getByText(oldMarker,{exact:true}).count()===0,"Delayed old history contaminated selected target");
            assert(await page.getByText(freshMarker,{exact:true}).isVisible(),"Current history lost");
            const currentBody=await currentResponse.json();
            assert(currentBody.conversationGeneration!==oldBody.conversationGeneration,"Distinct target generations missing");
            return {oldSession:activeTarget.backendSessionId,currentSession:otherTarget.backendSessionId,oldGeneration:oldBody.conversationGeneration,currentGeneration:currentBody.conversationGeneration};
          } finally {release.resolve();await page.unroute(oldPath);await fixture.command({type:"focus",index:activeIndex},"focusChanged");}
        });
        await scenario("4_upload_cancel_and_result_open",async()=>{
          await page.reload(); await composer.waitFor();
          const chooser=handled(page.waitForEvent("filechooser"));
          await page.getByTestId("attach-file-button").click();
          const uploaded=handled(page.waitForResponse(r=>r.url().endsWith("/api/v1/chat/attachments/upload")));
          const fileChooser=await chooser;
          const fileInput=await fileChooser.element();
          assert(await fileInput.getAttribute("data-testid")==="file-upload-input","Attach button opened the wrong file input");
          await page.getByTestId("file-upload-input").setInputFiles({name:"roundtrip.txt",mimeType:"text/plain",buffer:Buffer.from("HERDR_UPLOAD_BYTES")});
          const receipt=await (await uploaded).json(); assert(receipt.ok && receipt.data?.attachmentId,"Real upload receipt missing");
          assert(receipt.data.sha256===createHash("sha256").update("HERDR_UPLOAD_BYTES").digest("hex") && receipt.data.sizeBytes===Buffer.byteLength("HERDR_UPLOAD_BYTES"),"Uploaded byte receipt mismatch");
          await page.getByTestId("chat-composer-attachments-staging").waitFor({state:"hidden"});
          const sentFile=handled(page.waitForResponse(r=>r.url().endsWith("/api/v1/chat/send")));
          await page.getByTestId("send-button").click(); const sentResponse=await sentFile;
          assert(sentResponse.request().postDataJSON().draft.attachments.some(a=>a.attachmentId===receipt.data.attachmentId),"Attachment omitted from actual turn");
          assert((await sentResponse.json()).data?.stage==="accepted","Attachment turn not accepted");
          await page.getByRole("button",{name:"Remove attachment roundtrip.txt",exact:true}).waitFor({state:"hidden"});
          const captured=signal(), release=signal(), uploadSettled=signal();
          await page.route("**/api/v1/chat/attachments/upload",async route=>{
            try {
              const response=await route.fetch(); assert(response.ok(),"Initial chunk rejected"); captured.resolve();
              await deadline(release.promise,"upload release"); await route.fulfill({response}); uploadSettled.resolve();
            } catch(error) {captured.resolve(error);uploadSettled.resolve(error);}
          },{times:1});
          try {
            await page.getByTestId("file-upload-input").setInputFiles({name:"cancel.txt",mimeType:"text/plain",buffer:Buffer.alloc(40000,65)});
            const chunkError=await deadline(captured.promise,"first actual chunk staged"); if(chunkError)throw chunkError;
            await page.getByTestId("chat-composer-attachments-staging").waitFor();
            const cancelled=handled(page.waitForResponse(r=>r.url().endsWith("/api/v1/chat/attachments/cancel")));
            await page.getByRole("button",{name:"Remove attachment cancel.txt",exact:true}).click();
            const cancellation=await (await cancelled).json(); assert(cancellation.ok && cancellation.cleaned,"Cancellation did not remove server staging");
            release.resolve(); await deadline(uploadSettled.promise,"cancelled upload settled");
            await page.getByTestId("chat-composer-attachments-staging").waitFor({state:"hidden"});
            assert(await page.getByRole("button",{name:"Remove attachment cancel.txt",exact:true}).count()===0,"Cancelled upload entered draft");
          } finally {release.resolve();await page.unroute("**/api/v1/chat/attachments/upload");}
          const target=activeTarget;
          const listingResponse=await post("/api/v1/files/results/list",{target});
          const listing=await listingResponse.json();
          assert(listingResponse.ok() && listing.ok===true,"Result-file listing failed");
          assert(Array.isArray(listing.files) && listing.files.length>0,"Result-file listing was empty");
          const forbiddenFields=["path","relativePath","filePath","localPath","directory","sha256","bytes"];
          for(const file of listing.files) {
            assert(file && typeof file.fileId==="string" && file.fileId.length>0 && typeof file.displayName==="string",
              "Result listing entry lacks fileId or displayName");
            assert(Object.keys(file).length===2 && Object.keys(file).every(key=>key==="fileId" || key==="displayName"),
              "Result listing entry contains fields beyond fileId and displayName");
            for(const field of forbiddenFields) assert(!(field in file),"Result listing exposed forbidden field: "+field);
          }
          const listedFile=listing.files[0];
          const minted=await post("/api/v1/files/preview/token",{target,fileId:listedFile.fileId});
          const capability=await minted.json(); assert(minted.ok() && capability.token,"Result capability missing");
          const preview=await context.newPage();
          try {const response=await preview.goto(ready.gatewayUrl+"/api/v1/files/preview/"+capability.token);assert(response?.ok(),"Result open failed");assert((await preview.locator("body").innerText()).trim()==="HERDR_RESULT_FILE_ROUND_TRIP","Result bytes differ");}
          finally {await preview.close();}
          const unknown=await post("/api/v1/files/preview/token",{target,fileId:"unknown-result-file-id"});
          assert(unknown.status()===404,"Unknown result file id was not rejected");
          const hostile=await post("/api/v1/files/preview/token",{target,fileId:"../outside.txt"});
          assert(hostile.status()===404,"Hostile result file id was not rejected");
          let crossTargetStatus=null;
          if(otherTarget) {
            const otherListingResponse=await post("/api/v1/files/results/list",{target:otherTarget});
            const otherListing=await otherListingResponse.json();
            assert(otherListingResponse.ok() && otherListing.ok===true && Array.isArray(otherListing.files),"Second target result-file listing failed");
            if(otherListing.files.length>0) {
              const crossTarget=await post("/api/v1/files/preview/token",{target,fileId:otherListing.files[0].fileId});
              crossTargetStatus=crossTarget.status();
              assert(crossTargetStatus===404,"Cross-target result file id was not rejected");
            }
          } else {
            // The server's different-target registry case is covered by attachment_api_tests.rs:711.
          }
          return {attachmentId:receipt.data.attachmentId,previewOpened:true,listedFileId:listedFile.fileId,cancelled:true,
            unknownFileIdStatus:unknown.status(),hostileFileIdStatus:hostile.status(),crossTargetStatus};
        });
        await scenario("5_distinct_terminal_modes",async()=>{
          await composer.fill("HERDR_CHAT_ONLY_DRAFT"); await page.getByTestId("remote-view-mode-terminal").click();
          const toggle=page.getByTestId("remote-terminal-input-mode-toggle"); await toggle.waitFor();
          if(await toggle.getAttribute("data-mode")==="direct")await toggle.click();
          const line=page.getByTestId("remote-terminal-line-input");await line.waitFor();
          assert(await line.inputValue()==="","Chat draft leaked into terminal line mode");
          await toggle.click();assert(await toggle.getAttribute("data-mode")==="direct","Direct mode unavailable");
          await page.getByTestId("remote-view-mode-chat").click();await observeValue(composer,"HERDR_CHAT_ONLY_DRAFT");
          return {draftPreserved:true};
        });
        report.scenarios[viewport.width+"_6_voice"]={...report.nativeSpeechGate};
        report.scenarios[viewport.width+"_7_bounded_output"]=validateConsumerReceipt(args["consumer-receipt"],candidate);
      } finally {await Promise.all([...pendingEvidence]); await context.close();}
    }
  } catch(error) {report.errors.push(error.message);}
  finally {
    if(browser)await browser.close().catch(error=>report.errors.push(error.message));
    if(fixture)try {report.cleanup=await fixture.close();} catch(error){report.errors.push(error.message);}
    if(profile)try {profile.cleanup();} catch(error){report.errors.push(error.message);}
    const results=Object.values(report.scenarios);
    const failed=report.errors.length>0 || results.some(s=>s.status==="FAIL") || !report.cleanup;
    const gates=[report.androidNativeGate,report.iosNativeGate,report.remoteHistoryGate];
    const countedResults=results.filter(s=>s.status!=="DEFERRED");
    const allStatuses=[...results,...gates,report.nativeSpeechGate];
    const summary={passed:allStatuses.filter(s=>s.status==="PASS").length,
      failed:allStatuses.filter(s=>s.status==="FAIL").length,
      unmetPrerequisite:allStatuses.filter(s=>s.status==="UNMET_PREREQUISITE").length,
      deferred:allStatuses.filter(s=>s.status==="DEFERRED").length};
    report.summary={...summary,text:`${summary.passed} passed, ${summary.failed} failed, ${summary.unmetPrerequisite} unmet-prerequisite, ${summary.deferred} deferred; voice is deferred by user decision with no device evidence`};
    report.overallStatus=failed ? "FAIL" : countedResults.length===14 && countedResults.every(s=>s.status==="PASS") && gates.every(s=>s.status==="PASS") ? "PASS" : "PREREQUISITES_OPEN";
    writeReport(args["evidence-dir"],report);
  }
  return report.overallStatus==="PASS" ? 0 : report.overallStatus==="FAIL" ? 1 : 2;
}
if(process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  runMobileAcceptance().then(code=>{process.exitCode=code;}).catch(error=>{console.error(error.message);process.exitCode=2;});
}
