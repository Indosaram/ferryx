// The desktop still publishes `signals.attentionRows` (older remotes read them), and the remote
// loader still validates them. Nothing renders them any more: this guards only the wire.
import { describe, expect, it } from "vitest";

import { PUBLISHED_ATTENTION_ROWS_CAP, sidebarSessionSignals } from "../components/sidebar/sidebarSessionSignals";
import type { AttentionRow } from "../features/ferryx/attention/attentionModel";
import { loadRemoteSidebar } from "./remoteSidebarSource";
import { sidebarModelFixture } from "./remoteSidebarTestUtils";

const ROWS = [{ workspaceId: "ws-ferryx", slug: "main", label: "main", projectLabel: "ferryx" }];
const row = (id: string): AttentionRow =>
  ({ id, revision: 1, workspaceId: "ws-ferryx", sessionId: `s-${id}`, state: "done", who: `Agent ${id}`, location: "ws-ferryx", at: 1 });

async function load(wireModel: unknown) {
  const body = new TextEncoder().encode(JSON.stringify({ model: wireModel }));
  return loadRemoteSidebar(async () => ({ status: 200, body }) as never);
}

describe("published attention rows on the wire", () => {
  it("publishes the desktop's first 50 rows, which the remote's validator accepts", async () => {
    const many = Array.from({ length: 60 }, (_, i) => row(`r${i}`));
    const built = sidebarModelFixture(ROWS);
    const res = await load({ ...built, signals: sidebarSessionSignals(built, { attentionRows: many }) });

    expect(res.kind === "ok" && res.model.source).toBe("desktop");
    const rows = res.kind === "ok" ? res.model.signals?.attentionRows : undefined;
    expect(rows).toEqual(many.slice(0, PUBLISHED_ATTENTION_ROWS_CAP));
    expect(PUBLISHED_ATTENTION_ROWS_CAP).toBe(50);
  });

  it("does not trust a snapshot whose backendSessionId is not a string", async () => {
    const built = sidebarModelFixture(ROWS);
    const wire = { ...built, signals: { unreadWorktreePaths: {}, agentStateByPath: {}, attentionRows: [{ ...row("x"), backendSessionId: 7 }] } };
    const res = await load(wire);

    // A malformed published model is replaced by the machine's own inventory model.
    expect(res.kind === "ok" && res.model.source).not.toBe("desktop");
  });
});
