import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

const openExternalUrl = vi.hoisted(() => vi.fn(async (_url: string) => undefined));
vi.mock("../lib/browserTauri", () => ({ openExternalUrl }));

import { FilePreviewNotebook } from "./FilePreviewNotebook";

describe("FilePreviewNotebook", () => {
  afterEach(cleanup);

  it("renders http links as buttons that open the system browser and other links as inert text", () => {
    const notebook = JSON.stringify({
      cells: [{ cell_type: "markdown", source: "[docs](https://example.com/x) [bad](javascript:alert(1))" }],
    });
    const { container } = render(<FilePreviewNotebook text={notebook} />);
    expect(container.querySelector("a")).toBeNull();
    const links = screen.getAllByTestId("notebook-link");
    expect(links).toHaveLength(1);
    const docs = links[0];
    expect(docs.tagName).toBe("BUTTON");
    expect(docs).toHaveAttribute("type", "button");
    fireEvent.click(docs);
    expect(openExternalUrl).toHaveBeenCalledWith("https://example.com/x");
    openExternalUrl.mockClear();
    const bad = screen.getByText("bad");
    expect(bad.tagName).toBe("SPAN");
    fireEvent.click(bad);
    expect(openExternalUrl).not.toHaveBeenCalled();
  });

  it("styles markdown lists and headings without a typography plugin and allows selection", () => {
    const notebook = JSON.stringify({
      cells: [{ cell_type: "markdown", source: "## Section\n\n- one\n- two\n\n`x`" }],
    });
    const { container } = render(<FilePreviewNotebook text={notebook} />);
    expect(screen.getByRole("heading", { level: 2 }).className).toContain("font-semibold");
    expect(container.querySelector("ul")?.className).toContain("list-disc");
    expect(container.querySelector("code")?.className).toContain("font-mono");
    expect(screen.getByTestId("file-preview-notebook")).toHaveClass("selectable");
  });

  it("does not fetch remote markdown images", () => {
    const notebook = JSON.stringify({
      cells: [{ cell_type: "markdown", source: "![pixel](https://tracker.example/p.png)" }],
    });
    const { container } = render(<FilePreviewNotebook text={notebook} />);
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByTestId("notebook-image-blocked")).toHaveTextContent("pixel");
  });

  it("strips ANSI colors from stream output", () => {
    const notebook = JSON.stringify({
      cells: [{
        cell_type: "code",
        execution_count: 1,
        source: "print()",
        outputs: [{ output_type: "stream", name: "stdout", text: "\u001b[32mok\u001b[0m" }],
      }],
    });
    render(<FilePreviewNotebook text={notebook} />);
    expect(screen.getByText("ok")).toBeInTheDocument();
  });

  it("renders invalid banner when parseNotebook returns null", () => {
    render(<FilePreviewNotebook text="not json" />);
    const invalidElem = screen.getByTestId("file-preview-notebook-invalid");
    expect(invalidElem).toBeInTheDocument();
    expect(screen.queryByTestId("file-preview-notebook")).not.toBeInTheDocument();
  });

  it("renders markdown headings and content properly", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "markdown",
          source: "# Main Notebook Title\n\nThis is a paragraph with **bold** text.",
        },
      ],
    });

    render(<FilePreviewNotebook text={notebook} />);
    const root = screen.getByTestId("file-preview-notebook");
    expect(root).toBeInTheDocument();

    const cell = screen.getAllByTestId("notebook-cell").filter((el) => el.getAttribute("data-kind") === "markdown")[0];
    expect(cell).toHaveAttribute("data-kind", "markdown");

    const heading = screen.getByRole("heading", { level: 1, name: "Main Notebook Title" });
    expect(heading).toBeInTheDocument();
    expect(screen.getByText(/This is a paragraph with/)).toBeInTheDocument();
  });

  it("does not render script inside markdown as an element", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "markdown",
          source: "# Safe markdown\n\n<script>alert('xss')</script>",
        },
      ],
    });

    const { container } = render(<FilePreviewNotebook text={notebook} />);
    const scriptTag = container.querySelector("script");
    expect(scriptTag).toBeNull();
  });

  it("renders code cells with execution count and gutter", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "code",
          execution_count: 42,
          source: ["def foo():\n", "    return 1"],
          outputs: [],
        },
      ],
    });

    render(<FilePreviewNotebook text={notebook} />);
    const cell = screen.getAllByTestId("notebook-cell").filter((el) => el.getAttribute("data-kind") === "code")[0];
    expect(cell).toHaveAttribute("data-kind", "code");
    expect(screen.getByText("In [42]:")).toBeInTheDocument();
    expect(screen.getByText(/def foo\(\):/)).toBeInTheDocument();
  });

  it("renders image output with data URI", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "code",
          execution_count: 1,
          source: "plot()",
          outputs: [
            {
              output_type: "display_data",
              data: {
                "image/png": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
              },
            },
          ],
        },
      ],
    });

    render(<FilePreviewNotebook text={notebook} />);
    const img = screen.getByAltText("Cell output");
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute(
      "src",
      "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
    );
  });

  it("renders error output in a red-tinted box with ANSI stripped", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "code",
          execution_count: 2,
          source: "1 / 0",
          outputs: [
            {
              output_type: "error",
              ename: "\u001b[1;31mZeroDivisionError\u001b[0m",
              evalue: "integer division by zero",
              traceback: [
                "\u001b[1;31mZeroDivisionError\u001b[0m: integer division by zero",
              ],
            },
          ],
        },
      ],
    });

    render(<FilePreviewNotebook text={notebook} />);
    expect(screen.getByText("ZeroDivisionError")).toBeInTheDocument();
    expect(screen.getByText("ZeroDivisionError: integer division by zero")).toBeInTheDocument();
    expect(screen.queryByText(/\\u001b/)).not.toBeInTheDocument();
  });

  it("ignores text/html output and never renders HTML elements from it", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "code",
          execution_count: 3,
          source: "df.head()",
          outputs: [
            {
              output_type: "execute_result",
              data: {
                "text/html": "<table id='dangerous-table'><tr><td>Injected</td></tr></table>",
              },
            },
          ],
        },
      ],
    });

    const { container } = render(<FilePreviewNotebook text={notebook} />);
    expect(container.querySelector("#dangerous-table")).toBeNull();
    expect(screen.queryByText("Injected")).not.toBeInTheDocument();
  });
});
