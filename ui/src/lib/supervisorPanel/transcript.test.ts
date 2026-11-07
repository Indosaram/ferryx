import { afterEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  desktop: false,
  mobile: false,
  ios: false,
  save: vi.fn(async (_name: string, _text: string) => "/home/me/Downloads/saved.txt"),
  share: vi.fn(async (_name: string, _text: string) => true),
}));
vi.mock("../tauri", () => ({
  isTauriRuntime: () => tauri.desktop,
  readSupervisorTranscriptPage: vi.fn(),
  saveSupervisorTranscriptFile: tauri.save,
  shareSupervisorTranscriptFile: tauri.share,
}));
vi.mock("../clientRuntime", () => ({
  isMobileCompanion: () => tauri.mobile,
  isIosCompanion: () => tauri.mobile && tauri.ios,
}));

import {
  exportTranscriptFile,
  formatTranscript,
  isReportMessage,
  parseTranscriptPage,
  readTranscriptAfter,
  reportExcerpt,
  REPORT_PREFIX,
  type TranscriptMessage,
  type TranscriptPage,
} from "./transcript";

const realCreate = Object.getOwnPropertyDescriptor(URL, "createObjectURL");
const realRevoke = Object.getOwnPropertyDescriptor(URL, "revokeObjectURL");
function restoreUrl(name: "createObjectURL" | "revokeObjectURL", original: PropertyDescriptor | undefined) {
  if (original) Object.defineProperty(URL, name, original);
  else delete (URL as unknown as Record<string, unknown>)[name];
}

afterEach(() => {
  tauri.desktop = false;
  tauri.mobile = false;
  tauri.ios = false;
  tauri.save.mockClear();
  tauri.share.mockClear();
  restoreUrl("createObjectURL", realCreate);
  restoreUrl("revokeObjectURL", realRevoke);
  vi.restoreAllMocks();
});

function message(id: number, role: TranscriptMessage["role"], text: string, requestId = `r${id}`): TranscriptMessage {
  return { id, role, text, evidence: [], requestId, createdAtMs: Date.UTC(2026, 10, 3, 9, 0, id) };
}

const PAIR = [message(1, "owner", "Retry every 5 min"), message(2, "supervisor", "Done: retrying every 5 min.", "reply:r1")];

describe("transcript page", () => {
  it("parses the stored page wire shape and refuses a malformed row", () => {
    const row = { id: 1, role: "owner", text: "hi", evidence: [], requestId: "a", createdAtMs: 5 };
    expect(parseTranscriptPage({ messages: [row], nextCursor: 1, hasMore: false })).toEqual({ messages: [row], nextCursor: 1, hasMore: false });
    expect(parseTranscriptPage({ messages: [{ id: 1, role: "stranger", text: "x", requestId: "a" }] })).toBeNull();
    expect(parseTranscriptPage({ rows: [] })).toBeNull();
  });

  it("reads every page after the cursor and stops when the cursor does not advance", async () => {
    const pages: TranscriptPage[] = [
      { messages: [PAIR[0]], nextCursor: 1, hasMore: true },
      { messages: [PAIR[1]], nextCursor: 2, hasMore: false },
    ];
    const load = vi.fn(async (cursor: number) => pages[cursor]);
    const read = await readTranscriptAfter(load, 0);
    expect(read.messages.map((m) => m.id)).toEqual([1, 2]);
    expect(read.cursor).toBe(2);
    expect(load.mock.calls.map(([cursor]) => cursor)).toEqual([0, 1]);

    const stuck = vi.fn(async () => ({ messages: [], nextCursor: 0, hasMore: true }));
    await readTranscriptAfter(stuck, 0);
    expect(stuck).toHaveBeenCalledTimes(1);
  });
});

describe("report excerpt", () => {
  const sections = Array.from({ length: 40 }, (_, i) => `## Section ${i + 1}\n\nMigration check ${i + 1}.`);
  const report = ["# F3 long report tok1", "", ...sections, "", "End of report tok1"].join("\n");

  it("shows the beginning and the last line of a long report, and a short one whole", () => {
    const excerpt = reportExcerpt(report);
    expect(excerpt.head.split("\n")[0]).toBe("F3 long report tok1");
    expect(excerpt.tail).toBe("End of report tok1");
    expect(reportExcerpt("one\ntwo")).toEqual({ head: "one\ntwo", tail: null });
  });

  it("recognizes a stored worker report by its request id", () => {
    expect(isReportMessage(message(3, "supervisor", report, `${REPORT_PREFIX}lane_event:9`))).toBe(true);
    expect(isReportMessage(PAIR[1])).toBe(false);
    expect(isReportMessage(message(4, "owner", report, `${REPORT_PREFIX}x`))).toBe(false);
  });
});

describe("transcript export", () => {
  it("writes prompt and Reply rows, oldest first, in the stored words", () => {
    const text = formatTranscript(PAIR);
    expect(text).toContain("Owner prompt\nRetry every 5 min");
    expect(text).toContain("Supervisor reply\nDone: retrying every 5 min.");
    expect(text.indexOf("Retry every 5 min")).toBeLessThan(text.indexOf("Done: retrying"));
  });

  it("downloads a text file in the remote web", async () => {
    const create = vi.fn((_blob: Blob) => "blob:transcript");
    Object.assign(URL, { createObjectURL: create, revokeObjectURL: vi.fn() });
    let download: string | null = null;
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      download = this.download;
    });
    const result = await exportTranscriptFile(PAIR, Date.UTC(2026, 10, 3, 9, 30, 0));
    expect(result).toEqual({ ok: true, summary: "Downloaded supervisor-transcript-2026-11-03T09-30-00-000Z.txt" });
    expect(download).toBe("supervisor-transcript-2026-11-03T09-30-00-000Z.txt");
    const blob = create.mock.calls[0][0];
    expect(blob.type).toContain("text/plain");
    expect(blob.size).toBe(new TextEncoder().encode(formatTranscript(PAIR)).length);
  });

  it("saves through the desktop command and reports a refusal", async () => {
    tauri.desktop = true;
    expect(await exportTranscriptFile(PAIR, 0)).toEqual({ ok: true, summary: "Saved /home/me/Downloads/saved.txt" });
    expect(tauri.save.mock.calls[0][1]).toContain("Done: retrying every 5 min.");
    tauri.save.mockRejectedValueOnce(new Error("disk full"));
    expect(await exportTranscriptFile(PAIR, 0)).toEqual({ ok: false, message: "disk full" });
  });

  it("the iPhone companion shares the file through the native share sheet, never the browser path", async () => {
    tauri.desktop = true;
    tauri.mobile = true;
    tauri.ios = true;
    const create = vi.fn(() => "blob:never");
    Object.defineProperty(URL, "createObjectURL", { configurable: true, writable: true, value: create });
    const result = await exportTranscriptFile(PAIR, Date.UTC(2026, 10, 3, 9, 30, 0));
    expect(result).toEqual({ ok: true, summary: "Shared supervisor-transcript-2026-11-03T09-30-00-000Z.txt" });
    expect(tauri.share).toHaveBeenCalledTimes(1);
    expect(tauri.share.mock.calls[0][0]).toBe("supervisor-transcript-2026-11-03T09-30-00-000Z.txt");
    expect(tauri.share.mock.calls[0][1]).toContain("Owner prompt\nRetry every 5 min");
    expect(tauri.share.mock.calls[0][1]).toContain("Supervisor reply\nDone: retrying every 5 min.");
    expect(create).not.toHaveBeenCalled();
    expect(tauri.save).not.toHaveBeenCalled();
  });

  it("a dismissed share sheet, a failed stage and the Android companion are never reported as saved", async () => {
    tauri.desktop = true;
    tauri.mobile = true;
    tauri.ios = true;
    tauri.share.mockResolvedValueOnce(false);
    expect(await exportTranscriptFile(PAIR, 0)).toEqual({ ok: false, message: "Export cancelled. Nothing was saved." });
    tauri.share.mockRejectedValueOnce(new Error("the export file was not staged: disk full"));
    expect(await exportTranscriptFile(PAIR, 0)).toEqual({ ok: false, message: "the export file was not staged: disk full" });
    tauri.ios = false;
    const android = await exportTranscriptFile(PAIR, 0);
    expect(android.ok).toBe(false);
    expect(tauri.share).toHaveBeenCalledTimes(2);
    expect(tauri.save).not.toHaveBeenCalled();
  });
});
