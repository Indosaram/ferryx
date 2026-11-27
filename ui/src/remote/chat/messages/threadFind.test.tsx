import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it } from "vitest";
import { useEffect } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MobileChatMessage } from "../MobileChatMessage";
import {
  ThreadFindProvider,
  countMatches,
  highlightMatches,
  useThreadFind,
  useThreadFindHotkey,
} from "./threadFind";

afterEach(cleanup);

// The hotkey lives beside the provider the way the list pane would mount it.
function FindProbe() {
  return (
    <ThreadFindProvider>
      <HotkeyMount />
      <MobileChatMessage id="m1" role="assistant" content="the quick brown fox" />
      <MobileChatMessage id="m2" role="assistant" content="another quick message" />
    </ThreadFindProvider>
  );
}

function HotkeyMount() {
  useThreadFindHotkey(useThreadFind());
  return null;
}

describe("threadFind", () => {
  it("settles when a message reports no matches", () => {
    // A message with no matches has no entry in the count map. Registering it must not
    // mint a new map every render: that re-renders, re-registers, and never settles.
    let renders = 0;
    function NoMatchProbe() {
      const find = useThreadFind()!;
      renders += 1;
      if (renders > 50) throw new Error("thread find registration never settled");
      useEffect(() => {
        find.registerMatches("none", 0);
      }, [find]);
      return null;
    }
    render(
      <ThreadFindProvider>
        <HotkeyMount />
        <NoMatchProbe />
        <MobileChatMessage id="m1" role="assistant" content="the quick brown fox" />
      </ThreadFindProvider>,
    );
    expect(screen.queryByTestId("thread-find-bar")).not.toBeInTheDocument();
    expect(renders).toBeLessThan(10);
  });

  it("counts case-insensitive matches", () => {
    expect(countMatches("Quick quick QUICK", "quick")).toBe(3);
    expect(countMatches("abc", "zzz")).toBe(0);
    expect(countMatches("anything", "")).toBe(0);
  });

  it("highlights matches and marks the active ordinal", () => {
    render(<div>{highlightMatches("a quick quick fox", "quick", "m", 1, { messageId: "m", ordinal: 2 })}</div>);
    const marks = screen.getAllByTestId("thread-find-match");
    expect(marks).toHaveLength(2);
    expect(marks[1]).toHaveAttribute("data-active", "true");
    expect(marks[0]).not.toHaveAttribute("data-active");
  });

  it("opens with Cmd/Ctrl+F, walks next/prev across messages, and closes on Escape", async () => {
    render(<FindProbe />);
    fireEvent.keyDown(window, { key: "f", metaKey: true });
    const input = screen.getByTestId("thread-find-input");
    await act(async () => {
      fireEvent.change(input, { target: { value: "quick" } });
    });
    expect(screen.getByTestId("thread-find-count")).toHaveTextContent("1/2");
    expect(screen.getAllByTestId("thread-find-match")).toHaveLength(2);
    fireEvent.click(screen.getByTestId("thread-find-next"));
    expect(screen.getByTestId("thread-find-count")).toHaveTextContent("2/2");
    fireEvent.click(screen.getByTestId("thread-find-prev"));
    expect(screen.getByTestId("thread-find-count")).toHaveTextContent("1/2");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("thread-find-bar")).not.toBeInTheDocument();
  });
});
