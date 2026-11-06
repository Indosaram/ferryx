import assert from "node:assert/strict";
import { connectData, attachAndWatch } from "./daemon-lifetime/lib/client.mjs";
import { data } from "./daemon-lifetime/lib/wire.mjs";

const runtime = process.argv[2];
const cwd = process.argv[3];
if (!runtime || !cwd) throw new Error("Pass isolated runtime and repository");
const { conn } = await connectData(runtime, "linux");
let session;
let output;
try {
  const identity = await conn.call({ type: "referenceChatIdentity" });
  assert.equal(identity.type, "referenceChatIdentityOk");
  await conn.call(data.registerWorkspace("chat-qa", cwd));
  const spawned = await conn.call(data.spawnLocal({ clientRequestId: crypto.randomUUID(), workspaceId: "chat-qa", cwd, shell: "/bin/bash" }));
  assert.equal(spawned.type, "spawnOk", JSON.stringify(spawned));
  session = spawned.sessionId;
  output = await attachAndWatch(runtime, "linux", session);
  const marker = `CHAT_SOCKET_${crypto.randomUUID()}`;
  const observed = output.waitFor(marker);
  const request = {
    kind: "submit", requestId: crypto.randomUUID(), registryId: null,
    target: { target: { ...identity.identity, backendSessionId: session }, providerSessionId: null },
    params: { text: `printf '%s%s\\n' 'CHAT_SOCKET_' '${marker.slice(12)}'`, origin: "chat" },
  };
  const first = await conn.call({ type: "referenceChat", request });
  assert.equal(first.type, "referenceChatOk", JSON.stringify(first));
  assert.equal(first.reply.status, 200, JSON.stringify(first));
  try {
    await observed;
  } catch (error) {
    console.error(JSON.stringify({ receipt: first, output: output.text() }));
    throw error;
  }
  const duplicate = await conn.call({ type: "referenceChat", request });
  assert.deepEqual(duplicate, first);
  console.log(JSON.stringify({ result: "PASS", session, receipt: first.reply.body, outputObserved: true }));
} finally {
  output?.close();
  if (session) await conn.call(data.close(session));
  conn.close();
}
