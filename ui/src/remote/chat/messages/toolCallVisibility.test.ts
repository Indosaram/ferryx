import "@testing-library/jest-dom/vitest";
import { describe, expect, it } from "vitest";
import {
  filterPartsForVisibility,
  isToolCallVisibility,
  zenErrorParts,
  zenHidesTurn,
} from "./toolCallVisibility";
import type { ReferencePart } from "../referenceTypes";

const text = (id: string): ReferencePart => ({ kind: "text", text: "hello", id });
const thinking = (id: string): ReferencePart => ({ kind: "thinking", text: "hmm", id });
const tool = (overrides: Partial<Extract<ReferencePart, { kind: "tool" }>> = {}): ReferencePart => ({
  kind: "tool",
  name: "bash",
  summary: "",
  input: "ls",
  output: "files",
  id: "t",
  ...overrides,
});
const task = (overrides: Partial<Extract<ReferencePart, { kind: "taskResult" }>["tasks"][number]> = {}) => ({
  id: "task",
  title: "Explore",
  status: "completed" as const,
  result: "done",
  ...overrides,
});
const tasks = (overrides: Partial<Extract<ReferencePart, { kind: "taskResult" }>> = {}): ReferencePart => ({
  kind: "taskResult",
  tasks: [task()],
  id: "tasks",
  ...overrides,
});
const diff: ReferencePart = {
  kind: "diff",
  patch: { ref: "p", byteLength: 4, digest: "d", truncated: false },
  id: "diff",
};

describe("toolCallVisibility", () => {
  it("validates the four reference modes", () => {
    expect(isToolCallVisibility("all")).toBe(true);
    expect(isToolCallVisibility("hide-successful")).toBe(true);
    expect(isToolCallVisibility("hide-all")).toBe(true);
    expect(isToolCallVisibility("zen")).toBe(true);
    expect(isToolCallVisibility("quiet")).toBe(false);
    expect(isToolCallVisibility(null)).toBe(false);
  });

  it("shows everything on all", () => {
    const parts = [text("a"), tool(), thinking("b"), diff];
    expect(filterPartsForVisibility(parts, "all")).toEqual(parts);
  });

  it("hide-successful keeps a running call and drops it once it succeeds", () => {
    const parts = [tool({ status: "running" }), tool({ id: "done", status: "completed" }), text("a")];
    const visible = filterPartsForVisibility(parts, "hide-successful");
    expect(visible).toEqual([parts[0], text("a")]);
  });

  it("hide-successful always keeps a failed call", () => {
    const parts = [tool({ status: "failed", error: true })];
    expect(filterPartsForVisibility(parts, "hide-successful")).toEqual(parts);
    expect(filterPartsForVisibility(parts, "hide-all")).toEqual(parts);
  });

  it("hide-all drops every call but keeps thinking", () => {
    const parts = [tool({ status: "running" }), thinking("t"), tasks(), diff, text("a")];
    expect(filterPartsForVisibility(parts, "hide-all")).toEqual([thinking("t"), text("a")]);
  });

  it("zen keeps only the errors of an in-between reply", () => {
    const parts = [text("a"), tool({ status: "failed", error: true }), tool({ id: "ok", status: "completed" })];
    expect(zenErrorParts(parts)).toEqual([parts[1]]);
  });

  it("zen hides assistant in-between replies but never user messages, and find overrides it", () => {
    expect(zenHidesTurn("assistant", false, false)).toBe(true);
    expect(zenHidesTurn("assistant", true, false)).toBe(false);
    expect(zenHidesTurn("user", false, false)).toBe(false);
    expect(zenHidesTurn("assistant", false, true)).toBe(false);
  });
});
