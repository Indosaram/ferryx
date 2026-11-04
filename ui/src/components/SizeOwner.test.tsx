import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SizeOwner, isPhoneSizeOwner } from "./SizeOwner";

const bridge = vi.hoisted(() => ({
  listener: null as null | ((event: { payload: { sessionId: string; owner: string; cols: number; rows: number } }) => void),
  resize: vi.fn(async () => {}),
}));
vi.mock("../lib/tauri", () => ({
  listenToThisWindow: vi.fn(async (_event: string, listener: NonNullable<typeof bridge.listener>) => { bridge.listener = listener; return () => {}; }),
  describeTerminal: vi.fn(async () => ({ sizeOwner: "phone:1", cols: 60, rows: 30 })),
  resizeTerminal: bridge.resize,
  reportDesktopWindowFocus: vi.fn(async () => {}),
}));

describe("SizeOwner", () => {
  it("recognizes only non-desktop ownership", () => {
    expect(isPhoneSizeOwner(undefined)).toBe(false);
    expect(isPhoneSizeOwner("desktop")).toBe(false);
    expect(isPhoneSizeOwner("phone:1")).toBe(true);
  });

  it("claims the matching session and observes ownership restoration", async () => {
    await act(async () => { render(<SizeOwner sessionId="pty-1" />); });
    await act(async () => { fireEvent.click(screen.getByRole("button")); });
    expect(bridge.resize).toHaveBeenCalledWith({ sessionId: "pty-1", cols: 60, rows: 30 });
    act(() => bridge.listener?.({ payload: { sessionId: "other", owner: "desktop", cols: 120, rows: 40 } }));
    expect(screen.getByRole("button")).toBeTruthy();
    act(() => bridge.listener?.({ payload: { sessionId: "pty-1", owner: "desktop", cols: 120, rows: 40 } }));
    expect(screen.queryByRole("button")).toBeNull();
  });
});
