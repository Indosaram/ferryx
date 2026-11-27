/**
 * The workspace's seam forwarding (plan chat-t3-parity-20261125, lane G5 item 2).
 *
 * The composer and message renderer belong to other lanes, so the props that reach them are
 * recorded here rather than rendered: what this lane promises is that an owner's seam arrives
 * at the child unchanged, and that an owner who supplies nothing lets the child's own default
 * stand (an explicit `undefined` would beat it).
 */
import React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { MobileChatWorkspace } from "../MobileChatWorkspace";
import type { ChipSources, PermissionControl } from "../composer/chipTypes";

const seen = vi.hoisted(() => ({
  composer: [] as Record<string, unknown>[],
  message: [] as Record<string, unknown>[],
  insertCitation: null as null | ((citation: { messageId: string; text: string }) => void),
}));

vi.mock("../MobileChatComposer", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../MobileChatComposer")>();
  const MockComposer = React.forwardRef<unknown, Record<string, unknown>>((props, ref) => {
    seen.composer.push(props);
    React.useImperativeHandle(ref, () => ({
      clearDraft: () => undefined,
      focus: () => undefined,
      insertCitation: (citation: { messageId: string; text: string }) => seen.insertCitation?.(citation),
    }));
    return React.createElement("div", { "data-testid": "mock-composer" });
  });
  MockComposer.displayName = "MockComposer";
  return { ...actual, MobileChatComposer: MockComposer };
});

vi.mock("../MobileChatMessage", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../MobileChatMessage")>();
  return {
    ...actual,
    MobileChatMessage: (props: Record<string, unknown>) => {
      seen.message.push(props);
      return React.createElement("div", { "data-testid": "mock-message" }, String(props.content ?? ""));
    },
  };
});

window.HTMLElement.prototype.scrollIntoView = vi.fn();

const messages = [
  { id: "u1", role: "user" as const, content: "first", timestamp: 1 },
  { id: "a1", role: "assistant" as const, content: "answer", timestamp: 2 },
];

afterEach(() => {
  cleanup();
  seen.composer.length = 0;
  seen.message.length = 0;
  seen.insertCitation = null;
});

describe("MobileChatWorkspace seam forwarding", () => {
  it("hands the composer every parity seam the owner supplies", () => {
    const chipSources: ChipSources = { models: [{ id: "m1", kind: "model", label: "Opus" }] };
    const permissionControl: PermissionControl = { modes: ["supervised", "auto"], current: "supervised", onChange: vi.fn() };
    const onEditQueued = vi.fn();
    const onRemoveQueued = vi.fn();
    const onSendQueuedNow = vi.fn();
    const onInterruptQueued = vi.fn();
    render(
      <MobileChatWorkspace
        messages={messages}
        onSendMessage={vi.fn()}
        chipSources={chipSources}
        permissionControl={permissionControl}
        sentPrompts={["first"]}
        onEditQueued={onEditQueued}
        onRemoveQueued={onRemoveQueued}
        onSendQueuedNow={onSendQueuedNow}
        onInterruptQueued={onInterruptQueued}
      />,
    );
    const props = seen.composer.at(-1) ?? {};
    expect(props.chipSources).toBe(chipSources);
    expect(props.permissionControl).toBe(permissionControl);
    expect(props.sentPrompts).toEqual(["first"]);
    expect(props.onEditQueued).toBe(onEditQueued);
    expect(props.onRemoveQueued).toBe(onRemoveQueued);
    expect(props.onSendQueuedNow).toBe(onSendQueuedNow);
    expect(props.onInterruptQueued).toBe(onInterruptQueued);
  });

  it("adds no optional seam when the owner supplies none, so the children keep their defaults", () => {
    render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);
    const props = seen.composer.at(-1) ?? {};
    for (const key of ["chipSources", "permissionControl", "sentPrompts", "onEditQueued", "onRemoveQueued", "onSendQueuedNow", "onInterruptQueued"]) {
      expect(props).not.toHaveProperty(key);
    }
    const turn = seen.message.at(-1) ?? {};
    for (const key of ["onOpenFile", "onOpenExternal"]) {
      expect(turn).not.toHaveProperty(key);
    }
    // Citation is the one seam the workspace always offers: it is the workspace's own routing.
    expect(turn.onCiteSelection).toBeTypeOf("function");
  });

  it("routes a cited selection into the composer and still calls the owner's own handler", () => {
    const onCiteSelection = vi.fn();
    const intoComposer = vi.fn();
    seen.insertCitation = intoComposer;
    render(
      <MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} onCiteSelection={onCiteSelection} />,
    );
    const turn = seen.message.at(-1) ?? {};
    const cite = turn.onCiteSelection as (citation: { messageId: string; text: string }) => void;
    expect(cite).toBeTypeOf("function");
    const citation = { messageId: "a1", text: "the quoted answer" };
    cite(citation);
    expect(intoComposer).toHaveBeenCalledWith(citation);
    expect(onCiteSelection).toHaveBeenCalledWith(citation);
  });

  it("forwards the file and link seams to every turn", () => {
    const onOpenFile = vi.fn();
    const onOpenExternal = vi.fn();
    render(
      <MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} onOpenFile={onOpenFile} onOpenExternal={onOpenExternal} />,
    );
    const turn = seen.message.at(-1) ?? {};
    expect(turn.onOpenFile).toBe(onOpenFile);
    expect(turn.onOpenExternal).toBe(onOpenExternal);
  });

  it("forwards tool-call visibility and the announcement and image seams to every turn", () => {
    const onSaveImage = vi.fn();
    const onCopyImage = vi.fn();
    render(
      <MobileChatWorkspace
        messages={messages}
        onSendMessage={vi.fn()}
        toolCallVisibility="hide-successful"
        announceLevel="status-and-messages"
        onSaveImage={onSaveImage}
        onCopyImage={onCopyImage}
      />,
    );
    const turn = seen.message.at(-1) ?? {};
    // The renderer owns the filter, so the mode itself must reach it: pre-applying it here
    // would lose the find-bar override and which of a turn's parts zen keeps.
    expect(turn.toolCallVisibility).toBe("hide-successful");
    expect(turn.announceLevel).toBe("status-and-messages");
    expect(turn.onSaveImage).toBe(onSaveImage);
    expect(turn.onCopyImage).toBe(onCopyImage);
  });

  it("leaves the renderer-owned seams absent when the owner supplies none", () => {
    render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);
    const turn = seen.message.at(-1) ?? {};
    for (const key of ["announceLevel", "onSaveImage", "onCopyImage"]) {
      expect(turn).not.toHaveProperty(key);
    }
  });
});
