import { afterEach, describe, expect, it } from "vitest";

import {
  PANE_CHAT_LAST_AGENT_STORAGE_KEY,
  PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY,
  chatSupportedAgents,
  isChatSupportedAgent,
  isToolCallVisibility,
  loadLastChatAgentId,
  loadToolCallVisibility,
  orderChatAgents,
  pickChatAgent,
  saveLastChatAgentId,
  saveToolCallVisibility,
} from "./paneChatPreferences";
import { findConflict, SHORTCUTS } from "./shortcuts";

const agent = (name: string) => ({ name, command: name, args: "" });

afterEach(() => localStorage.clear());

describe("paneChatPreferences", () => {
  it("offers only agents with a native history reader", () => {
    expect(isChatSupportedAgent("claude")).toBe(true);
    expect(isChatSupportedAgent("cursor-agent")).toBe(true);
    expect(isChatSupportedAgent("aider")).toBe(false);
    expect(isChatSupportedAgent("")).toBe(false);
    expect(chatSupportedAgents([agent("aider"), agent("codex"), agent("mystery")]).map((a) => a.name)).toEqual(["codex"]);
  });

  it("persists the last used agent under a ferryx.* key", () => {
    expect(PANE_CHAT_LAST_AGENT_STORAGE_KEY.startsWith("ferryx.")).toBe(true);
    expect(loadLastChatAgentId()).toBeNull();
    saveLastChatAgentId("codex");
    expect(localStorage.getItem(PANE_CHAT_LAST_AGENT_STORAGE_KEY)).toBe("codex");
    expect(loadLastChatAgentId()).toBe("codex");
  });

  it("orders last used, then default, then registry order", () => {
    const agents = [agent("claude"), agent("codex"), agent("pi")];
    expect(orderChatAgents(agents, "pi", "codex").map((a) => a.name)).toEqual(["pi", "codex", "claude"]);
    expect(orderChatAgents(agents, null, "codex").map((a) => a.name)).toEqual(["codex", "claude", "pi"]);
  });

  it("falls back when the last used agent is no longer offered", () => {
    expect(pickChatAgent([agent("claude"), agent("codex")], "pi", null)?.name).toBe("claude");
    expect(pickChatAgent([], "pi", null)).toBeNull();
  });

  it("binds tab.newChat to a chord free on both platforms", () => {
    const definition = SHORTCUTS.find((shortcut) => shortcut.id === "tab.newChat");
    expect(definition).toBeDefined();
    expect(findConflict("tab.newChat", definition!.binding, {}, true)).toBeNull();
    expect(findConflict("tab.newChat", definition!.binding, {}, false)).toBeNull();
  });

  it("persists tool calls in chat under a ferryx.* key and validates the stored value", () => {
    expect(PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY.startsWith("ferryx.")).toBe(true);
    expect(loadToolCallVisibility()).toBe("all");
    saveToolCallVisibility("zen");
    expect(localStorage.getItem(PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY)).toBe("zen");
    expect(loadToolCallVisibility()).toBe("zen");
    localStorage.setItem(PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY, "quiet");
    expect(loadToolCallVisibility()).toBe("all");
    expect(isToolCallVisibility("hide-successful")).toBe(true);
    expect(isToolCallVisibility("all")).toBe(true);
    expect(isToolCallVisibility("hide-all")).toBe(true);
    expect(isToolCallVisibility("other")).toBe(false);
  });
});
