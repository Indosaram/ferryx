import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ReferencePartList } from "./MobileChatComponents";

afterEach(cleanup);

describe("ReferencePartList prose lists", () => {
  it("keeps list markers on ordered and unordered lists in native turn markdown", () => {
    const { container } = render(
      <ReferencePartList
        parts={[
          { kind: "text", text: "Steps:\n\n1. Stop the daemon\n2. Rebuild\n3. Restart\n\nNotes:\n\n- one\n- two" },
        ]}
      />,
    );
    const ol = container.querySelector("ol");
    const ul = container.querySelector("ul");
    expect(ol).not.toBeNull();
    expect(ul).not.toBeNull();
    expect(ol!.className).toContain("list-decimal");
    expect(ol!.className).toContain("pl-4");
    expect(ul!.className).toContain("list-disc");
    expect(ul!.className).toContain("pl-4");
  });
});

describe("ReferencePartList cut tool output", () => {
  it("ui-messages-r4-1: offers 'Show the whole output' only when a whole-output handler is wired", () => {
    const part = {
      kind: "tool" as const,
      name: "bash",
      summary: "",
      input: "cat big.log",
      output: "head of the output",
      outputRef: "r1",
      outputSize: 90000,
    };

    const { unmount } = render(<ReferencePartList parts={[part]} />);
    fireEvent.click(screen.getByTestId("work-row"));
    expect(screen.getByTestId("reference-tool-output")).toHaveTextContent("head of the output");
    expect(screen.queryByTestId("reference-tool-whole-output")).not.toBeInTheDocument();
    unmount();

    const onRequestWholeOutput = vi.fn();
    render(<ReferencePartList parts={[part]} context={{ onRequestWholeOutput }} />);
    fireEvent.click(screen.getByTestId("work-row"));
    const button = screen.getByTestId("reference-tool-whole-output");
    expect(button).toHaveTextContent("Show the whole output (90000 characters)");
    fireEvent.click(button);
    expect(onRequestWholeOutput).toHaveBeenCalledWith("r1");
  });
});

describe("ReferencePartList linked markdown images", () => {
  it("ui-messages-r4-2: a linked badge renders one anchor to the link target, never a nested image anchor", () => {
    const { container } = render(
      <ReferencePartList
        parts={[{ kind: "text", text: "[![build](https://img.shields.io/b.svg)](https://ci.example/run/1)" }]}
      />,
    );
    expect(container.querySelectorAll("a a")).toHaveLength(0);
    const anchors = Array.from(container.querySelectorAll("a"));
    expect(anchors.map((a) => a.getAttribute("href"))).toEqual(["https://ci.example/run/1"]);
    expect(anchors[0]).toContainElement(screen.getByTestId("markdown-remote-image"));
    expect(screen.getByTestId("markdown-remote-image").tagName).toBe("SPAN");
  });
});
