import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HttpAttachmentUploader } from "./httpUploader.ts";
const root = await mkdtemp(join(tmpdir(), "ferryx-design-upload-"));
const bytes = new Uint8Array([137,80,78,71,13,10,26,10]);
const hash = createHash("sha256").update(bytes).digest("hex");
const target = { hostId:"fixture-remote",ownerId:"owner",epoch:"1",backendSessionId:"session" };
const server = Bun.serve({ hostname:"127.0.0.1", port:0, async fetch(request) {
  const received = new Uint8Array(await request.arrayBuffer());
  assert.deepEqual(JSON.parse(decodeURIComponent(request.headers.get("X-Ferryx-Target"))), target);
  assert.equal(createHash("sha256").update(received).digest("hex"), request.headers.get("X-Ferryx-Sha256"));
  await writeFile(join(root,"target-readable.png"), received, {mode:0o600});
  return Response.json({ok:true,data:{hostId:target.hostId,attachmentId:"opaque-fixture",sha256:hash,sizeBytes:received.length,mediaType:"image/png"}});
} });
try {
  const uploader = new HttpAttachmentUploader(new URL(`http://127.0.0.1:${server.port}/staging`));
  const receipt = await uploader.stage(target,bytes,hash,"request");
  assert.equal(receipt.sha256,hash); assert.deepEqual(new Uint8Array(await readFile(join(root,"target-readable.png"))),bytes);
  console.log(JSON.stringify({status:"passed", boundary:"actual HTTP byte transfer and receiver-owned private file", bytes:bytes.length, remoteSsh:false, productionStaging:false}));
} finally { await server.stop(true); await rm(root,{recursive:true}); console.log(JSON.stringify({cleanup:"loopback server stopped and private receiver directory removed"})); }
