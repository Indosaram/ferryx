import { StrictMode } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const renderCounter = vi.hoisted(() => ({ sourceLines: 0 }));
vi.mock("../lib/cn", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/cn")>();
  return {
    ...actual,
    cn: (...args: Parameters<typeof actual.cn>) => {
      // SourceLine is the only caller that builds this exact content class.
      if (args[0] === "min-w-0 flex-1") renderCounter.sourceLines += 1;
      return actual.cn(...args);
    },
  };
});

const clipboardMock = vi.hoisted(() => ({ copyTextToClipboard: vi.fn(async (_text: string) => true) }));
vi.mock("../lib/clipboard", () => clipboardMock);

import { FilePreviewText } from "./FilePreviewText";
import type {
  FilePreviewChildAsset,
  FilePreviewMarkdownCapability,
  FilePreviewPayload,
  FilePreviewTextProps,
} from "../lib/filePreviewTypes";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function payloadOf(over: Partial<FilePreviewPayload> = {}): FilePreviewPayload {
  const text = over.text ?? "hello\n";
  return {
    handle: "h-1",
    displayName: "notes.txt",
    kind: "text",
    byteLength: text.length,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text,
    lineCount: text.split("\n").length,
    target: null,
    resolvedPath: null,
    ...over,
  };
}

function childAsset(): FilePreviewChildAsset {
  return {
    handle: "child-1",
    displayName: "logo.png",
    kind: "image",
    byteLength: 1024,
    mediaType: "image/png",
    mediaUrl: "http://127.0.0.1:53211/asset/child-1",
  };
}

function capabilityOf(over: Partial<FilePreviewMarkdownCapability> = {}): FilePreviewMarkdownCapability {
  return {
    requestImage: vi.fn(async () => childAsset()),
    requestDocument: vi.fn(),
    openExternalUrl: vi.fn(),
    remainingChildHandles: 32,
    ...over,
  };
}

function propsOf(over: Partial<FilePreviewTextProps> = {}): FilePreviewTextProps {
  return {
    payload: payloadOf(),
    generation: 1,
    onFailure: vi.fn(),
    target: null,
    sourceMode: false,
    onSourceModeChange: vi.fn(),
    markdown: capabilityOf(),
    ...over,
  } as FilePreviewTextProps;
}

function markdownProps(text: string, over: Partial<FilePreviewTextProps> = {}): FilePreviewTextProps {
  return propsOf({ payload: payloadOf({ text, kind: "markdown", displayName: "README.md" }), ...over });
}

describe("FilePreviewText review fixes", () => {
  it("re-renders only the lines whose matches changed on a search keystroke", () => {
    const text = [...Array.from({ length: 200 }, () => "aaaa"), "needle"].join("\n");
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text }) })} />);
    const input = screen.getByTestId("file-preview-search-input");

    fireEvent.change(input, { target: { value: "nee" } });
    renderCounter.sourceLines = 0;
    fireEvent.change(input, { target: { value: "need" } });

    expect(screen.getByTestId("file-preview-search-status")).toHaveTextContent("1 of 1");
    expect(renderCounter.sourceLines).toBeLessThan(10);
  });

  it("gives every heading the id its TOC entry links to, including setext and after nested headings", () => {
    const md = ["Intro", "=====", "", "> # Quoted", "", "Setup", "-----", "", "## Install", ""].join("\n");
    const { container } = render(
      <StrictMode>
        <FilePreviewText {...markdownProps(md)} />
      </StrictMode>,
    );
    fireEvent.click(screen.getByTestId("file-preview-md-toc-toggle"));
    const toc = screen.getByTestId("file-preview-md-toc");
    const items = within(toc).getAllByRole("button");
    expect(items.map((item) => item.textContent)).toEqual(["Intro", "Setup", "Install"]);

    for (const item of items) {
      const slug = item.getAttribute("data-testid")!.replace("file-preview-md-toc-item-", "");
      const heading = container.querySelector(`[id="${slug}"]`);
      expect(heading?.textContent).toBe(item.textContent);
    }
  });

  it("copies through the shared clipboard helper", async () => {
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text: "copy me\n" }) })} />);
    fireEvent.click(screen.getByTestId("file-preview-copy"));
    await waitFor(() => expect(clipboardMock.copyTextToClipboard).toHaveBeenCalledWith("copy me\n"));
    expect(await screen.findByText("Copied")).toBeInTheDocument();
  });

  it("reports a failed copy when the clipboard helper cannot write", async () => {
    clipboardMock.copyTextToClipboard.mockResolvedValueOnce(false);
    const props = propsOf();
    render(<FilePreviewText {...props} />);
    fireEvent.click(screen.getByTestId("file-preview-copy"));
    await waitFor(() => expect(props.onFailure).toHaveBeenCalledTimes(1));
  });

  it("moves to the next and previous match with Enter and Shift+Enter", () => {
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text: "ab\nab\nab\n" }) })} />);
    const input = screen.getByTestId("file-preview-search-input");
    fireEvent.change(input, { target: { value: "ab" } });
    const status = screen.getByTestId("file-preview-search-status");
    expect(status).toHaveTextContent("1 of 3");

    fireEvent.keyDown(input, { key: "Enter" });
    expect(status).toHaveTextContent("2 of 3");
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(status).toHaveTextContent("3 of 3");
  });

  it("exposes toggle state through aria-pressed", () => {
    render(<FilePreviewText {...markdownProps("# Title\n")} />);
    const toc = screen.getByTestId("file-preview-md-toc-toggle");
    expect(toc).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(toc);
    expect(toc).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("file-preview-md-width-toggle")).toHaveAttribute("aria-pressed");
  });

  it("renders highlighted markup-looking source as inert text", () => {
    const source = '<img src=x onerror=alert(1)>\n';
    const { container } = render(
      <FilePreviewText {...propsOf({ payload: payloadOf({ text: source, displayName: "page.html" }) })} />,
    );
    expect(container.querySelector(".hljs-line")).not.toBeNull();
    expect(container.querySelectorAll("img")).toHaveLength(0);
    expect(screen.getByTestId("file-preview-line-1")).toHaveTextContent("<img src=x onerror=alert(1)>");
  });

  it("acquires a local image once under StrictMode and still paints it", async () => {
    const requestImage = vi.fn(async () => childAsset());
    render(
      <StrictMode>
        <FilePreviewText {...markdownProps("![logo](./img/logo.png)\n", { markdown: capabilityOf({ requestImage }) })} />
      </StrictMode>,
    );
    expect(await screen.findByAltText("logo")).toHaveAttribute("src", childAsset().mediaUrl);
    expect(requestImage).toHaveBeenCalledTimes(1);
  });

  it("keeps an already-loaded image when the child-handle budget reaches zero", async () => {
    const requestImage = vi.fn(async () => childAsset());
    const md = "![logo](./img/logo.png)\n";
    // Like the controller's capability, one object per generation whose budget
    // is a live getter that drops as children are granted.
    let remaining = 32;
    const capability = capabilityOf({ requestImage });
    Object.defineProperty(capability, "remainingChildHandles", { get: () => remaining });
    const { rerender } = render(<FilePreviewText {...markdownProps(md, { markdown: capability })} />);
    expect(await screen.findByAltText("logo")).toHaveAttribute("src", childAsset().mediaUrl);

    remaining = 0;
    rerender(<FilePreviewText {...markdownProps(md, { markdown: capability })} />);
    expect(await screen.findByAltText("logo")).toHaveAttribute("src", childAsset().mediaUrl);
    expect(screen.queryByTestId("file-preview-rejected-image")).toBeNull();
    expect(requestImage).toHaveBeenCalledTimes(1);
  });

  it("still blocks an image that never requested a handle once the budget is spent", () => {
    const requestImage = vi.fn(async () => childAsset());
    render(
      <FilePreviewText
        {...markdownProps("![logo](./img/logo.png)\n", {
          markdown: capabilityOf({ requestImage, remainingChildHandles: 0 }),
        })}
      />,
    );
    expect(screen.getByTestId("file-preview-rejected-image")).toHaveAttribute("data-reason", "child-handle-limit");
    expect(requestImage).not.toHaveBeenCalled();
  });

  it("scrolls the TOC target inside its own document, not a same-id element elsewhere", () => {
    const decoy = document.createElement("h2");
    decoy.id = "install";
    const decoyScroll = vi.fn();
    decoy.scrollIntoView = decoyScroll;
    document.body.prepend(decoy);
    try {
      const { container } = render(<FilePreviewText {...markdownProps("# Intro\n\n## Install\n")} />);
      const heading = container.querySelector('[id="install"]') as HTMLElement;
      const headingScroll = vi.fn();
      heading.scrollIntoView = headingScroll;
      fireEvent.click(screen.getByTestId("file-preview-md-toc-toggle"));
      fireEvent.click(screen.getByTestId("file-preview-md-toc-item-install"));
      expect(headingScroll).toHaveBeenCalledWith({ block: "start" });
      expect(decoyScroll).not.toHaveBeenCalled();
    } finally {
      decoy.remove();
    }
  });

  it("keeps the caret marker on a line that also has search matches", () => {
    render(
      <FilePreviewText
        {...propsOf({ payload: payloadOf({ text: "hello world\n" }), target: { line: 1, col: 3 } })}
      />,
    );
    expect(screen.getByTestId("file-preview-caret")).toBeInTheDocument();
    fireEvent.change(screen.getByTestId("file-preview-search-input"), { target: { value: "hello" } });
    expect(screen.getByTestId("file-preview-match-0")).toBeInTheDocument();
    expect(screen.getByTestId("file-preview-caret")).toBeInTheDocument();
    expect(screen.getByTestId("file-preview-line-1")).toHaveTextContent("hello world");
  });

  it("announces search status politely", () => {
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text: "ab\n" }) })} />);
    fireEvent.change(screen.getByTestId("file-preview-search-input"), { target: { value: "ab" } });
    expect(screen.getByTestId("file-preview-search-status")).toHaveAttribute("aria-live", "polite");
  });

  it("marks the toolbar and TOC as hidden from print and keeps content selectable", () => {
    render(<FilePreviewText {...markdownProps("# Title\n")} />);
    fireEvent.click(screen.getByTestId("file-preview-md-toc-toggle"));
    expect(screen.getByTestId("file-preview-text-toolbar")).toHaveAttribute("data-print-hide");
    expect(screen.getByTestId("file-preview-md-toc")).toHaveAttribute("data-print-hide");
    expect(screen.getByTestId("file-preview-markdown")).toHaveClass("selectable");
  });

  it("makes the source view selectable", () => {
    render(<FilePreviewText {...propsOf()} />);
    expect(screen.getByTestId("file-preview-source")).toHaveClass("selectable");
  });
});
