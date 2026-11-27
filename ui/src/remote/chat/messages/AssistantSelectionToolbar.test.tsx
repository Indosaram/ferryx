import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useRef } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { AssistantSelectionToolbar, type CiteSelection } from "./AssistantSelectionToolbar";

afterEach(cleanup);

/**
 * The toolbar and the prose it watches live in one tree, the way the turn body
 * mounts them: rendering into document.body would clear the anchor element.
 */
function Body({ onCiteSelection }: { onCiteSelection?: (citation: CiteSelection) => void }) {
  const ref = useRef<HTMLDivElement | null>(null);
  return (
    <div>
      <div ref={ref} data-testid="assistant-body">
        <p>selectable agent words</p>
      </div>
      <AssistantSelectionToolbar messageId="m1" anchorRef={ref} onCiteSelection={onCiteSelection} />
    </div>
  );
}

/** Select the assistant body and announce it the way the browser does. */
function selectBody() {
  const body = screen.getByTestId("assistant-body");
  act(() => {
    const range = document.createRange();
    range.selectNodeContents(body);
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
  });
}

describe("AssistantSelectionToolbar", () => {
  it("appears on a selection inside the body and cites {messageId, text}", () => {
    const onCiteSelection = vi.fn();
    render(<Body onCiteSelection={onCiteSelection} />);
    expect(screen.queryByTestId("cite-selection-button")).not.toBeInTheDocument();

    selectBody();

    fireEvent.click(screen.getByTestId("cite-selection-button"));
    expect(onCiteSelection).toHaveBeenCalledWith({
      messageId: "m1",
      text: "selectable agent words",
    } satisfies CiteSelection);
  });

  it("stays hidden without an onCiteSelection handler", () => {
    render(<Body />);
    selectBody();
    expect(screen.queryByTestId("cite-selection-button")).not.toBeInTheDocument();
  });
});
