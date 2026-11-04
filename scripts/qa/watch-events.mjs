// Bun-only isolated-daemon event/DescribeSession probe. Never spawns or kills a daemon.
import { parseArgs } from "node:util";
import { join } from "node:path";

const { values } = parseArgs({ options: {
  runtime: { type: "string" }, kind: { type: "string" }, describe: { type: "string" },
} });
const runtime = values.runtime ?? process.env.FERRYX_RUNTIME_DIR;
if (!runtime) throw new Error("--runtime or FERRYX_RUNTIME_DIR is required (isolated daemon only)");
let pending = "";
const request = values.describe
  ? { type: "describeSession", sessionId: values.describe }
  : { type: "subscribeRemoteEvents" };
await Bun.connect({ unix: join(runtime, "daemon.sock"), socket: {
  open(socket) { socket.write(`${JSON.stringify(request)}\n`); },
  data(socket, bytes) {
    pending += new TextDecoder().decode(bytes, { stream: true });
    for (;;) {
      const end = pending.indexOf("\n");
      if (end < 0) break;
      const event = JSON.parse(pending.slice(0, end));
      pending = pending.slice(end + 1);
      if (values.describe || !values.kind || event.event === values.kind) {
        console.log(JSON.stringify(event));
      }
      if (values.describe) socket.end();
    }
  },
  error(_socket, error) { console.error(error); process.exitCode = 1; },
} });
