/**
 * The answer-set attachment store (lane G6, t3code question-attachments parity).
 * Authored, NOT EXECUTED — execution override via scripts/build-remote.sh.
 */
import { describe, expect, it } from "vitest";
import {
  PROMPT_ATTACH_MAX_FILES,
  ReferenceAnswerAttachmentStore,
  promptAttachmentDraftKey,
} from "./referencePromptAttach";
import type { ReferenceFileReceipt } from "../referenceTypes";

function file(name: string, bytes: number, type = "text/plain"): File {
  return new File([new Uint8Array(bytes)], name, { type });
}

function receipt(name: string): ReferenceFileReceipt {
  return {
    receipt: {
      hostId: "h1",
      attachmentId: `att-${name}`,
      sha256: "ab",
      sizeBytes: 3,
      mediaType: "text/plain",
    },
    displayName: name,
    mentionText: `@uploads/${name} `,
  };
}

function key(name = "p1"): string {
  return promptAttachmentDraftKey(name);
}

describe("promptAttachmentDraftKey", () => {
  it("keys one answer set per prompt", () => {
    expect(promptAttachmentDraftKey("p1")).toBe("p1");
    expect(promptAttachmentDraftKey("p2")).not.toBe(promptAttachmentDraftKey("p1"));
  });

  it("separates the same question id by the pane it was asked in", () => {
    // prompt ids are content hashes: the same question in another session/host hashes the same
    expect(promptAttachmentDraftKey("abc", "session-a")).not.toBe(
      promptAttachmentDraftKey("abc", "session-b"),
    );
    expect(promptAttachmentDraftKey("abc", "session-a")).toContain("session-a");
    expect(promptAttachmentDraftKey("abc", "session-a")).toContain("abc");
  });
});

describe("ReferenceAnswerAttachmentStore add", () => {
  it("takes the files it can and refuses the rest per file, naming each refusal", () => {
    const store = new ReferenceAnswerAttachmentStore();
    const { added, refusals } = store.add(key(), [
      file("notes.txt", 4),
      file("boom.exe", 4, "application/x-msdownload"),
    ]);
    expect(added).toHaveLength(1);
    expect(added[0]?.displayName).toBe("notes.txt");
    expect(added[0]?.status).toBe("uploading");
    expect(refusals).toHaveLength(1);
    expect(refusals[0]?.name).toBe("boom.exe");
    expect(refusals[0]?.reason).toContain("does not accept");
  });

  it("refuses a file over the composer's per-file byte limit", () => {
    const store = new ReferenceAnswerAttachmentStore();
    const { added, refusals } = store.add(key(), [file("big.bin", 11 * 1024 * 1024)]);
    expect(added).toHaveLength(0);
    expect(refusals[0]?.reason).toContain("byte limit");
  });

  it("carries at most the server's per-turn file cap in one answer set", () => {
    const store = new ReferenceAnswerAttachmentStore();
    // the server refuses the fifth file of a turn (`ATTACHMENT_MAX_FILES_PER_TURN`)
    expect(PROMPT_ATTACH_MAX_FILES).toBe(4);
    const full = [...Array(PROMPT_ATTACH_MAX_FILES)].map((_, index) => file(`f${index}.txt`, 1));
    expect(store.add(key(), full).added).toHaveLength(PROMPT_ATTACH_MAX_FILES);
    const over = store.add(key(), [file("one-more.txt", 1)]);
    expect(over.added).toHaveLength(0);
    expect(over.refusals[0]?.reason).toContain("at most 4");
  });

  it("keeps the composer's byte budget across the whole answer set", () => {
    const store = new ReferenceAnswerAttachmentStore();
    // two files at the per-file ceiling fit; the next byte over the set budget refuses
    store.add(key(), [file("a.bin", 10 * 1024 * 1024), file("b.bin", 10 * 1024 * 1024)]);
    const { added, refusals } = store.add(key(), [file("c.bin", 4)]);
    expect(added).toHaveLength(0);
    expect(refusals[0]?.reason).toContain("bytes");
  });
});

describe("ReferenceAnswerAttachmentStore upload state", () => {
  it("binds receipts to the files that asked for them, in order", () => {
    const store = new ReferenceAnswerAttachmentStore();
    const { added } = store.add(key(), [file("a.txt", 3), file("b.txt", 3)]);
    store.markReady(key(), added.map((each) => each.id), [receipt("a.txt"), receipt("b.txt")]);
    const set = store.get(key());
    expect(set.every((each) => each.status === "ready")).toBe(true);
    expect(store.readyReceipts(key()).map((each) => each.displayName)).toEqual(["a.txt", "b.txt"]);
    expect(store.hasUploading(key())).toBe(false);
    expect(store.hasFailed(key())).toBe(false);
  });

  it("refuses to answer when the upload did not return every path", () => {
    const store = new ReferenceAnswerAttachmentStore();
    const { added } = store.add(key(), [file("a.txt", 3), file("b.txt", 3)]);
    store.markReady(key(), added.map((each) => each.id), [receipt("a.txt")]);
    expect(store.hasFailed(key())).toBe(true);
  });

  it("marks a rejected upload failed with a plain-language reason", () => {
    const store = new ReferenceAnswerAttachmentStore();
    const { added } = store.add(key(), [file("a.txt", 3)]);
    store.markFailed(key(), added.map((each) => each.id), "the host refused it");
    const failed = store.get(key())[0];
    expect(failed?.status).toBe("failed");
    expect(failed?.error).toBe("the host refused it");
    expect(failed?.file).toBeInstanceOf(File);
    expect(store.hasFailed(key())).toBe(true);
  });

  it("moves a failed upload back to uploading for its retry, keeping the file", () => {
    const store = new ReferenceAnswerAttachmentStore();
    const { added } = store.add(key(), [file("a.txt", 3)]);
    const id = added[0]?.id ?? "";
    store.markFailed(key(), [id], "x");
    store.markRetrying(key(), id);
    const retrying = store.get(key())[0];
    expect(retrying?.status).toBe("uploading");
    expect(retrying?.error).toBeNull();
    expect(retrying?.file.name).toBe("a.txt");
  });

  it("removes one file without touching the others", () => {
    const store = new ReferenceAnswerAttachmentStore();
    const { added } = store.add(key(), [file("a.txt", 3), file("b.txt", 3)]);
    store.remove(key(), added[0]?.id ?? "");
    expect(store.get(key()).map((each) => each.displayName)).toEqual(["b.txt"]);
  });

  it("empties the set when the answer takes the files", () => {
    const store = new ReferenceAnswerAttachmentStore();
    store.add(key(), [file("a.txt", 3)]);
    store.clear(key());
    expect(store.get(key())).toHaveLength(0);
    expect(store.hasUploading(key())).toBe(false);
  });
});

describe("one answer set per prompt", () => {
  it("keeps two prompts' answer sets apart", () => {
    const store = new ReferenceAnswerAttachmentStore();
    store.add(key("p1"), [file("a.txt", 3)]);
    store.add(key("p2"), [file("b.txt", 3)]);
    expect(store.get(key("p1")).map((each) => each.displayName)).toEqual(["a.txt"]);
    expect(store.get(key("p2")).map((each) => each.displayName)).toEqual(["b.txt"]);
    store.clear(key("p1"));
    expect(store.get(key("p2"))).toHaveLength(1);
  });

  it("tells subscribers when a set changes", () => {
    const store = new ReferenceAnswerAttachmentStore();
    let changes = 0;
    const unsubscribe = store.subscribe(() => {
      changes += 1;
    });
    store.add(key(), [file("a.txt", 3)]);
    store.remove(key(), store.get(key())[0]?.id ?? "");
    unsubscribe();
    store.add(key(), [file("b.txt", 3)]);
    expect(changes).toBe(2);
    store.clear(key());
  });

  it("drops every answer set of a pane whose requests resolved, and no other pane's", () => {
    const store = new ReferenceAnswerAttachmentStore();
    const mine = promptAttachmentDraftKey("q1", "pane-a");
    const sibling = promptAttachmentDraftKey("q2", "pane-a");
    const other = promptAttachmentDraftKey("q1", "pane-b");
    store.add(mine, [file("a.txt", 3)]);
    store.add(sibling, [file("b.txt", 3)]);
    store.add(other, [file("c.txt", 3)]);
    let changes = 0;
    store.subscribe(() => {
      changes += 1;
    });
    store.clearScope("pane-a");
    expect(store.get(mine)).toHaveLength(0);
    expect(store.get(sibling)).toHaveLength(0);
    expect(store.get(other)).toHaveLength(1);
    expect(changes).toBe(1);
    store.clearScope("nothing-here");
    expect(changes).toBe(1);
    store.clear(other);
  });
});
