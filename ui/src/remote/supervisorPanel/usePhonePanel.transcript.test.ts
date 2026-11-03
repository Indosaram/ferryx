import { describe, expect, it, vi } from "vitest";

import type { SupervisorFetch } from "../remoteSupervisor";
import { transcriptLoaderFor } from "./usePhonePanel";

const page = { messages: [{ id: 4, role: "supervisor", text: "Reply", evidence: [], requestId: "reply:a", createdAtMs: 1 }], nextCursor: 4, hasMore: false };

describe("transcriptLoaderFor", () => {
  it("reads the stored page from the existing transcript route at the cursor", async () => {
    const fetchLike = vi.fn<SupervisorFetch>(async () => ({ status: 200, body: JSON.stringify(page) }));
    const loaded = await transcriptLoaderFor(fetchLike)(3);
    expect(fetchLike).toHaveBeenCalledWith("/api/v1/supervisor/transcript?cursor=3");
    expect(loaded.messages[0].text).toBe("Reply");
    expect(loaded.nextCursor).toBe(4);
  });

  it("refuses a failed read and a body that is not a page", async () => {
    await expect(transcriptLoaderFor(async () => ({ status: 503, body: "" }))(0)).rejects.toThrow("503");
    await expect(transcriptLoaderFor(async () => ({ status: 200, body: "{}" }))(0)).rejects.toThrow("not a transcript page");
  });
});
