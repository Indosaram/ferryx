import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MermaidCodeBlock } from "./mermaidBlock";

afterEach(cleanup);

const mermaidMock = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn(),
}));

vi.mock("mermaid", () => ({ default: mermaidMock }));

describe("MermaidCodeBlock", () => {
  it("renders a diagram and falls back to code on a render error", async () => {
    mermaidMock.render.mockImplementation((_id: string, code: string) =>
      code.includes("bad")
        ? Promise.reject(new Error("Parse error"))
        : Promise.resolve({ svg: "<svg><g>diagram</g></svg>" }),
    );
    const { rerender, container } = render(<MermaidCodeBlock code={"graph TD\nA-->B"} />);
    await waitFor(() => {
      expect(screen.getByTestId("mermaid-diagram")).toHaveAttribute("data-state", "rendered");
    });
    expect(mermaidMock.initialize).toHaveBeenCalled();
    expect(container.querySelector(".mermaid-diagram-svg svg")).not.toBeNull();

    rerender(<MermaidCodeBlock code={"graph TD\nbad-->syntax"} />);
    await waitFor(() => {
      expect(screen.getByTestId("mermaid-fallback")).toBeInTheDocument();
    });
    expect(screen.getByTestId("mermaid-fallback")).toHaveTextContent("bad-->syntax");
  });

  it("falls back immediately for empty diagram code", () => {
    render(<MermaidCodeBlock code="   " />);
    expect(screen.getByTestId("mermaid-fallback")).toBeInTheDocument();
    expect(mermaidMock.render).not.toHaveBeenCalled();
  });

  it("keeps the code block's copy and wrap behind a source/diagram toggle", async () => {
    mermaidMock.render.mockImplementation(() => Promise.resolve({ svg: "<svg><g>diagram</g></svg>" }));
    const { container } = render(
      <MermaidCodeBlock code={"graph TD\nA-->B"} fallback={<span data-testid="plain-code">source</span>} />,
    );
    await waitFor(() => {
      expect(screen.getByTestId("mermaid-diagram")).toHaveAttribute("data-state", "rendered");
    });
    fireEvent.click(screen.getByTestId("mermaid-show-code"));
    // The reader asked for the source: the ordinary code block (copy, wrap) replaces the diagram.
    expect(screen.getByTestId("plain-code")).toBeInTheDocument();
    expect(screen.queryByTestId("mermaid-diagram")).not.toBeInTheDocument();
    // Scoped to the diagram's own node: the toggle's icon is an <svg> too, and it stays.
    expect(container.querySelector(".mermaid-diagram-svg")).toBeNull();

    fireEvent.click(screen.getByTestId("mermaid-show-diagram"));
    // Coming back re-draws into a fresh node: the diagram is really there again, not just a state flag.
    await waitFor(() => {
      expect(container.querySelector(".mermaid-diagram-svg svg")).not.toBeNull();
    });
    expect(screen.getByTestId("mermaid-diagram")).toHaveAttribute("data-state", "rendered");
  });

  it("draws the caller's code block, not a bare pre, when the diagram fails", async () => {
    mermaidMock.render.mockImplementation(() => Promise.reject(new Error("Parse error")));
    render(<MermaidCodeBlock code={"graph TD\nbad-->syntax"} fallback={<span data-testid="plain-code">source</span>} />);
    await waitFor(() => {
      expect(screen.getByTestId("plain-code")).toBeInTheDocument();
    });
  });

  it("keeps copy and wrap on a failed diagram in the chat's markdown pipeline", async () => {
    mermaidMock.render.mockImplementation(() => Promise.reject(new Error("Parse error")));
    const { ChatProseMarkdown } = await import("../MobileChatComponents");
    render(<ChatProseMarkdown text={"```mermaid\ngraph TD\nbad-->syntax\n```"} assistant />);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Copy code" })).toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Wrap" })).toBeInTheDocument();
    expect(screen.getByTestId("highlighted-code-lines")).toHaveTextContent("bad-->syntax");
  });
});
