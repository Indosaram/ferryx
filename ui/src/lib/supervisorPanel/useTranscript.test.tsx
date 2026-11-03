import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const exported = vi.hoisted(() => ({ run: vi.fn(async () => ({ ok: true, summary: "Saved" }) as const) }));
vi.mock("./transcript", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./transcript")>()),
  exportTranscriptFile: exported.run,
}));

import type { TranscriptPage } from "./transcript";
import { useTranscript } from "./useTranscript";

afterEach(() => exported.run.mockClear());

const ROW = { id: 1, role: "owner" as const, text: "hello", evidence: [], requestId: "a", createdAtMs: 1 };
const PAGE: TranscriptPage = { messages: [ROW], nextCursor: 1, hasMore: false };

describe("useTranscript", () => {
  it("refuses to export when the final read failed, even though older rows are on screen", async () => {
    const load = vi.fn<(cursor: number) => Promise<TranscriptPage>>().mockResolvedValueOnce(PAGE);
    const { result } = renderHook(() => useTranscript(load));
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.messages).toHaveLength(1);

    load.mockRejectedValueOnce(new Error("offline"));
    let outcome: Awaited<ReturnType<typeof result.current.exportFile>> | undefined;
    await act(async () => {
      outcome = await result.current.exportFile();
    });
    expect(outcome).toEqual({ ok: false, message: "The latest transcript could not be read, so nothing was exported. Try again." });
    expect(exported.run).not.toHaveBeenCalled();
    expect(result.current.messages).toHaveLength(1);
  });

  it("exports every stored row once the final read completes", async () => {
    const load = vi.fn<(cursor: number) => Promise<TranscriptPage>>().mockResolvedValueOnce(PAGE).mockResolvedValue({ messages: [], nextCursor: 1, hasMore: false });
    const { result } = renderHook(() => useTranscript(load));
    await act(async () => {
      await result.current.refresh();
    });
    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.exportFile();
    });
    expect(outcome).toEqual({ ok: true, summary: "Saved" });
    expect(exported.run).toHaveBeenCalledTimes(1);
    expect((exported.run.mock.calls[0] as unknown as [unknown[]])[0]).toHaveLength(1);
  });
});
