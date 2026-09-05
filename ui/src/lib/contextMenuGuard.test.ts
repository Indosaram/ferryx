import { afterEach, describe, expect, it } from "vitest";

import { installContextMenuGuard } from "./contextMenuGuard";

const cleanups: Array<() => void> = [];

function dispatchContextMenu(target: HTMLElement): MouseEvent {
  const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
  document.body.replaceChildren();
});

describe("installContextMenuGuard", () => {
  it("prevents the native context menu on non-editable desktop content", () => {
    cleanups.push(installContextMenuGuard(true));

    expect(dispatchContextMenu(document.body).defaultPrevented).toBe(true);
  });

  it.each([
    ["input", "input"],
    ["textarea", "textarea"],
  ])("allows the native context menu on an %s", (_name, tagName) => {
    cleanups.push(installContextMenuGuard(true));
    const element = document.createElement(tagName);
    document.body.append(element);

    expect(dispatchContextMenu(element).defaultPrevented).toBe(false);
  });

  it("allows the native context menu on contenteditable content", () => {
    cleanups.push(installContextMenuGuard(true));
    const contentEditable = document.createElement("div");
    contentEditable.setAttribute("contenteditable", "true");
    document.body.append(contentEditable);

    expect(dispatchContextMenu(contentEditable).defaultPrevented).toBe(false);
  });

  it("does not suppress the native context menu in remote mode", () => {
    cleanups.push(installContextMenuGuard(false));

    expect(dispatchContextMenu(document.body).defaultPrevented).toBe(false);
  });

  it("removes the desktop listener during cleanup", () => {
    const cleanup = installContextMenuGuard(true);
    cleanup();

    expect(dispatchContextMenu(document.body).defaultPrevented).toBe(false);
  });
});
