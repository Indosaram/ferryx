import { describe, expect, it, vi } from "vitest";
import {
  answerAttachSeam,
  citeIntoComposer,
  composerSeams,
  messageSeams,
} from "./seams";

describe("workspace parity seams", () => {
  it("forwards only the seams the owner actually supplies", () => {
    const sentPrompts = ["run the tests"];
    const onOpenFile = vi.fn();
    // An explicit `undefined` would beat the child's own default, so it must not be forwarded.
    expect(composerSeams({ sentPrompts, permissionControl: undefined })).toEqual({ sentPrompts });
    expect(messageSeams({ onOpenFile, onOpenExternal: undefined })).toEqual({ onOpenFile });
    expect(composerSeams({})).toEqual({});
  });

  it("routes a citation into the composer handle and still tells the owner", () => {
    const insertCitation = vi.fn();
    const notify = vi.fn();
    citeIntoComposer({ insertCitation }, { messageId: "m1", text: "the anchor is kept" }, notify);
    expect(insertCitation).toHaveBeenCalledWith({ messageId: "m1", text: "the anchor is kept" });
    expect(notify).toHaveBeenCalledWith({ messageId: "m1", text: "the anchor is kept" });
  });

  it("still tells the owner when the mounted composer cannot take a citation", () => {
    const notify = vi.fn();
    citeIntoComposer(null, { messageId: "m1", text: "quote" }, notify);
    citeIntoComposer({}, { messageId: "m2", text: "quote" }, notify);
    expect(notify).toHaveBeenCalledTimes(2);
  });

  it("adds no answer-attach prop unless a staging route exists", () => {
    expect(answerAttachSeam(undefined)).toEqual({});
    const upload = vi.fn(async () => []);
    expect(answerAttachSeam(upload)).toEqual({ onAttachAnswerFiles: upload });
  });

  it("keys the answer set to the pane that asked, and only alongside an upload route", () => {
    const upload = vi.fn(async () => []);
    // A scope with no callback would name an answer set the card can never fill.
    expect(answerAttachSeam(undefined, "pane-a")).toEqual({});
    // Without a scope the card falls back to the scope its own callback pins.
    expect(answerAttachSeam(upload, null)).toEqual({ onAttachAnswerFiles: upload });
    expect(answerAttachSeam(upload, "")).toEqual({ onAttachAnswerFiles: upload });
    expect(answerAttachSeam(upload, "pane-a")).toEqual({
      onAttachAnswerFiles: upload,
      answerScope: "pane-a",
    });
  });

  it("forwards the renderer-owned message seams, including the ones with falsey-free defaults", () => {
    const onSaveImage = vi.fn();
    const onCopyImage = vi.fn();
    const seams = messageSeams({
      toolCallVisibility: "hide-successful",
      announceLevel: "status-only",
      onSaveImage,
      onCopyImage,
    });
    expect(seams).toEqual({
      toolCallVisibility: "hide-successful",
      announceLevel: "status-only",
      onSaveImage,
      onCopyImage,
    });
    // `zen` and `all` are real modes: neither may be dropped as a falsey default.
    expect(messageSeams({ toolCallVisibility: "all" }).toolCallVisibility).toBe("all");
    expect(messageSeams({ toolCallVisibility: "zen" }).toolCallVisibility).toBe("zen");
  });
});
