import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MobileChatMessage } from "../MobileChatMessage";
import { isExternalLinkHref, parseFileLinkHref } from "./markdownLinks";

afterEach(cleanup);

describe("markdown file links", () => {
  it("parses path, :line, and #L line forms, and rejects external URLs", () => {
    expect(parseFileLinkHref("src/app.ts")).toEqual({ path: "src/app.ts", line: undefined });
    expect(parseFileLinkHref("src/app.ts:42")).toEqual({ path: "src/app.ts", line: 42 });
    expect(parseFileLinkHref("src/app.ts#L7")).toEqual({ path: "src/app.ts", line: 7 });
    expect(parseFileLinkHref("https://example.com")).toBeNull();
    expect(parseFileLinkHref("")).toBeNull();
    expect(isExternalLinkHref("https://example.com")).toBe(true);
    expect(isExternalLinkHref("mailto:a@b.c")).toBe(true);
    expect(isExternalLinkHref("src/app.ts")).toBe(false);
  });

  it("does not read an in-page anchor, a query, or a protocol-relative URL as a file", () => {
    expect(parseFileLinkHref("#the-section")).toBeNull();
    expect(parseFileLinkHref("?q=1")).toBeNull();
    expect(parseFileLinkHref("//host/x")).toBeNull();
  });

  it("renders a local file link as a button that calls onOpenFile, never an anchor", () => {
    const onOpenFile = vi.fn();
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="See [the parser](src/app.ts:42) for details."
        onOpenFile={onOpenFile}
      />,
    );
    const link = screen.getByTestId("markdown-file-link");
    expect(link.tagName).toBe("BUTTON");
    expect(link).toHaveAttribute("data-path", "src/app.ts");
    expect(link).toHaveAttribute("data-line", "42");
    fireEvent.click(link);
    expect(onOpenFile).toHaveBeenCalledWith("src/app.ts", 42);
    expect(screen.queryByText("the parser", { selector: "a" })).not.toBeInTheDocument();
  });

  it("routes an external link through onOpenExternal instead of the browser default", () => {
    const onOpenExternal = vi.fn();
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="[docs](https://example.com/guide)"
        onOpenExternal={onOpenExternal}
      />,
    );
    const link = screen.getByTestId("markdown-external-link");
    fireEvent.click(link, { preventDefault: () => {} });
    expect(onOpenExternal).toHaveBeenCalledWith("https://example.com/guide");
  });
});
