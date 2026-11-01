// Runs inside the staged tree built by autopilot-redraft.test.ts (argv[2] = tree src dir).
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const src = process.argv[2]!;
const mode = process.argv[3]!;
const load = (name: string) => import(pathToFileURL(join(src, name)).href);
const S = await load("state.ts");
await load("slack.ts"); // creates report_outbox, as every production entry point does
const mutations: string[] = [];
try {
  if (mode === "seed") {
    for (const { chat_ref, draft = "old draft", ...item } of JSON.parse(process.argv[4]!)) {
      S.upsertNew(item);
      S.update(item.id, { status: "pending_approval", chat_ref, draft });
    }
  } else if (mode === "processing" || mode === "restore" || mode === "flush") {
    // REDRAFT_NO_PARENT leaves the card legacy: no hub-owned parent receipt exists.
    if (!process.env.REDRAFT_NO_PARENT)
      S.db.query("INSERT OR IGNORE INTO report_outbox(id,request,item_id,approval,ref,state,reason) VALUES('parent','{}',NULL,1,'D1:100.1','delivered','42')").run();
    const { trace } = await load("trace.ts");
    const server = Bun.listen<{ input: string }>({
      unix: process.env.FERRYX_HUB_CONTROL_SOCKET!,
      socket: {
        open(socket) { socket.data = { input: "" }; },
        data(socket, bytes) {
          socket.data.input += Buffer.from(bytes).toString();
          if (!socket.data.input.includes("\n")) return;
          const request = JSON.parse(socket.data.input);
          if (request.op === "report_adopt") {
            if (process.env.REDRAFT_ADOPT) {
              trace("hub", "report_adopt");
              socket.end(JSON.stringify({ status: "ok", op: "report_adopt", adoption: "adopted",
                outbox_id: 42, channel: request.channel, ts: request.ts }) + "\n");
            } else socket.end(JSON.stringify({ status: "pending", op: "report_adopt" }) + "\n");
            return;
          }
          trace("hub", request.change.kind);
          mutations.push(request.mutation_id);
          // REDRAFT_HUB_DELIVERY replays the hub's pending/uncertain/suppressed mutation receipts.
          socket.end(JSON.stringify({ status: "ok", op: "report_mutate", outbox_id: request.outbox_id,
            mutation_id: request.mutation_id, delivery: process.env.REDRAFT_HUB_DELIVERY ?? "delivered" }) + "\n");
        },
      },
    });
    const slack = await load("slack.ts");
    try {
      if (mode === "processing") await slack.markProcessing("D1:100.1", "bid");
      else if (mode === "restore") await slack.restoreButtons("D1:100.1");
      else await slack.flushReports();
    } finally { server.stop(true); }
  } else {
    const { draftWishketProposal } = await load("collect.ts");
    await draftWishketProposal(S.get(mode)!);
  }
  console.log(JSON.stringify({
    items: S.db.query("SELECT id,status,chat_ref,verdict,draft FROM items ORDER BY rowid").all(),
    reports: S.db.query("SELECT item_id,approval,request FROM report_outbox ORDER BY rowid").all(),
    log: S.db.query("SELECT item_id,event,detail FROM log ORDER BY rowid").all(),
    intents: S.db.query("SELECT card,item_id,mutation_id,state FROM report_card_intents ORDER BY seq").all(),
    mutations,
  }));
} finally { S.db.close(); }
