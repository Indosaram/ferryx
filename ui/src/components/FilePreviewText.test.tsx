import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FilePreviewText } from "./FilePreviewText";
import type {
  FilePreviewChildAsset,
  FilePreviewMarkdownCapability,
  FilePreviewPayload,
  FilePreviewTextProps,
} from "../lib/filePreviewTypes";

afterEach(() => {
  cleanup();
});

function payloadOf(over: Partial<FilePreviewPayload> = {}): FilePreviewPayload {
  const text = over.text ?? "hello\nworld\n";
  return {
    handle: "h-1",
    displayName: "notes.txt",
    kind: "text",
    byteLength: new TextEncoder().encode(text).length,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text,
    lineCount: text.split("\n").length,
    target: null,
    ...over,
    // `text` is resolved above so callers can override it without recomputing bytes.
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

function childAsset(over: Partial<FilePreviewChildAsset> = {}): FilePreviewChildAsset {
  return {
    handle: "child-1",
    displayName: "logo.png",
    kind: "image",
    byteLength: 1024,
    mediaType: "image/png",
    mediaUrl: "http://127.0.0.1:53211/asset/child-1",
    ...over,
  };
}

function propsOf(over: Partial<FilePreviewTextProps> = {}): FilePreviewTextProps {
  return {
    payload: payloadOf(),
    generation: 1,
    onReload: vi.fn(),
    onExternalOpen: vi.fn(),
    onFailure: vi.fn(),
    target: null,
    sourceMode: false,
    onSourceModeChange: vi.fn(),
    markdown: null,
    ...over,
  };
}

function markdownProps(source: string, over: Partial<FilePreviewTextProps> = {}): FilePreviewTextProps {
  return propsOf({
    payload: payloadOf({ kind: "markdown", displayName: "README.md", text: source }),
    markdown: capabilityOf(),
    ...over,
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("FilePreviewText source view", () => {
  it("renders every line with a 1-based number and escapes markup literally", () => {
    const text = "<script>window.__pwned = 1</script>\n안녕하세요 world\n";
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text }) })} />);

    const source = screen.getByTestId("file-preview-source");
    expect(within(source).getAllByTestId(/^file-preview-line-\d+$/)).toHaveLength(3);
    expect(within(source).getByText("<script>window.__pwned = 1</script>")).toBeInTheDocument();
    expect(within(source).getByText("안녕하세요 world")).toBeInTheDocument();
    expect(source.querySelector("script")).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
    expect(screen.getByTestId("file-preview-line-number-1")).toHaveTextContent("1");
    expect(screen.getByTestId("file-preview-line-number-3")).toHaveTextContent("3");
  });

  it("never mounts an editing surface", () => {
    const { container } = render(<FilePreviewText {...propsOf()} />);

    expect(container.querySelectorAll("textarea")).toHaveLength(0);
    expect(container.querySelectorAll("[contenteditable]")).toHaveLength(0);
    expect(screen.getByTestId("file-preview-source")).toHaveAttribute("aria-readonly", "true");
  });

  it("places the caret at a Unicode scalar column and reports clamping", () => {
    const text = "👩x\nsecond\n";
    const { rerender } = render(
      <FilePreviewText {...propsOf({ payload: payloadOf({ text }), target: { line: 1, col: 2 } })} />,
    );

    expect(screen.getByTestId("file-preview-caret")).toHaveAttribute("data-offset", "2");
    expect(screen.getByTestId("file-preview-location")).toHaveTextContent("Ln 1, Col 2");
    expect(screen.queryByTestId("file-preview-target-clamped")).toBeNull();

    rerender(
      <FilePreviewText {...propsOf({ payload: payloadOf({ text }), target: { line: 900, col: 900 } })} />,
    );

    expect(screen.getByTestId("file-preview-location")).toHaveTextContent("Ln 3, Col 1");
    expect(screen.getByTestId("file-preview-target-clamped")).toBeInTheDocument();
  });

  it("highlights and selects search matches without altering the document text", () => {
    const text = "Hello hello\n안녕 Hello\n";
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text }) })} />);
    const before = screen.getByTestId("file-preview-source").textContent;

    fireEvent.change(screen.getByTestId("file-preview-search-input"), { target: { value: "hello" } });

    expect(screen.getByTestId("file-preview-search-status")).toHaveTextContent("1 of 3");
    expect(screen.getAllByTestId(/^file-preview-match-/)).toHaveLength(3);
    expect(window.getSelection()?.toString()).toBe("Hello");
    expect(screen.getByTestId("file-preview-source").textContent).toBe(before);

    fireEvent.click(screen.getByTestId("file-preview-search-next"));
    expect(screen.getByTestId("file-preview-search-status")).toHaveTextContent("2 of 3");
    expect(window.getSelection()?.toString()).toBe("hello");

    fireEvent.click(screen.getByTestId("file-preview-search-previous"));
    expect(screen.getByTestId("file-preview-search-status")).toHaveTextContent("1 of 3");
  });

  it("copies the exact document text and reports clipboard failure through onFailure", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const props = propsOf({ payload: payloadOf({ text: "안녕\nworld\n" }) });
    const { rerender } = render(<FilePreviewText {...props} />);

    fireEvent.click(screen.getByTestId("file-preview-copy"));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("안녕\nworld\n"));

    const failing = vi.fn(async () => {
      throw new Error("denied");
    });
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: failing },
    });
    rerender(<FilePreviewText {...props} />);
    fireEvent.click(screen.getByTestId("file-preview-copy"));

    await waitFor(() => expect(props.onFailure).toHaveBeenCalledTimes(1));
    expect(props.onFailure).toHaveBeenCalledWith(
      expect.objectContaining({ reason: null, details: null }),
    );
  });

  it("offers no Markdown toggle for plain text", () => {
    render(<FilePreviewText {...propsOf()} />);
    expect(screen.queryByTestId("file-preview-source-toggle")).toBeNull();
    expect(screen.queryByTestId("file-preview-markdown")).toBeNull();
  });
});

describe("FilePreviewText Markdown rendering", () => {
  it("renders GFM structure as real elements", () => {
    render(
      <FilePreviewText
        {...markdownProps("# Title\n\n- one\n- two\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n")}
      />,
    );

    const doc = screen.getByTestId("file-preview-markdown");
    expect(within(doc).getByRole("heading", { level: 1, name: "Title" })).toBeInTheDocument();
    expect(within(doc).getAllByRole("listitem")).toHaveLength(2);
    expect(within(doc).getByRole("table")).toBeInTheDocument();
    expect(within(doc).getAllByRole("columnheader")).toHaveLength(2);
  });

  it("keeps raw HTML inert and literal", () => {
    const { container } = render(
      <FilePreviewText
        {...markdownProps('<script>window.__pwned = 1</script>\n\nplain <b>bold</b> text\n')}
      />,
    );

    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("b")).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
    expect(screen.getByTestId("file-preview-markdown").textContent).toContain(
      "<script>window.__pwned = 1</script>",
    );
    expect(screen.getByTestId("file-preview-markdown").textContent).toContain("<b>");
  });

  it("opens http(s) anchors only through the capability, never as navigable hrefs", () => {
    const props = markdownProps("[site](https://example.com/docs)\n");
    const { container } = render(<FilePreviewText {...props} />);

    const link = screen.getByTestId("file-preview-external-link");
    expect(container.querySelector("a[href]")).toBeNull();
    fireEvent.click(link);
    expect(props.markdown?.openExternalUrl).toHaveBeenCalledWith("https://example.com/docs");
  });

  it("refuses unsafe anchor schemes without any action", () => {
    const props = markdownProps("[boom](javascript:alert(1)) and [drive](file:///etc/passwd)\n");
    render(<FilePreviewText {...props} />);

    const rejected = screen.getAllByTestId("file-preview-rejected-link");
    expect(rejected).toHaveLength(2);
    fireEvent.click(rejected[0]);
    expect(props.markdown?.openExternalUrl).not.toHaveBeenCalled();
    expect(props.markdown?.requestDocument).not.toHaveBeenCalled();
    expect(rejected[0]).toHaveTextContent("boom");
  });

  it("escalates relative document links to a new preview request", () => {
    const props = markdownProps("[notes](./notes.md)\n");
    render(<FilePreviewText {...props} />);

    fireEvent.click(screen.getByTestId("file-preview-document-link"));
    expect(props.markdown?.requestDocument).toHaveBeenCalledWith("./notes.md");
    expect(props.markdown?.openExternalUrl).not.toHaveBeenCalled();
  });

  it("scrolls in-document fragments to the matching heading anchor", () => {
    const props = markdownProps("# Safe Section\n\n[jump](#safe-section)\n");
    render(<FilePreviewText {...props} />);

    const heading = screen.getByRole("heading", { level: 1, name: "Safe Section" });
    expect(heading).toHaveAttribute("id", "safe-section");
    const scrollIntoView = vi.spyOn(heading, "scrollIntoView").mockImplementation(() => {});

    fireEvent.click(screen.getByTestId("file-preview-fragment-link"));
    expect(scrollIntoView).toHaveBeenCalledTimes(1);
    expect(props.markdown?.openExternalUrl).not.toHaveBeenCalled();
  });

  it("resolves relative images through the parent capability", async () => {
    const asset = childAsset({ mediaUrl: "http://127.0.0.1:53211/asset/child-7" });
    const requestImage = vi.fn(async () => asset);
    const props = markdownProps("![logo](./img/logo.png)\n", {
      markdown: capabilityOf({ requestImage }),
    });
    render(<FilePreviewText {...props} />);

    await waitFor(() => expect(requestImage).toHaveBeenCalledWith("./img/logo.png"));
    const image = await screen.findByAltText("logo");
    expect(image).toHaveAttribute("src", "http://127.0.0.1:53211/asset/child-7");
    expect(requestImage).toHaveBeenCalledTimes(1);
  });

  it("never requests remote, data or traversal images", () => {
    const requestImage = vi.fn(async () => childAsset());
    const props = markdownProps(
      "![a](https://tracker.example/p.png)\n\n![b](data:image/png;base64,AAAA)\n\n![c](../secret.png)\n",
      { markdown: capabilityOf({ requestImage }) },
    );
    const { container } = render(<FilePreviewText {...props} />);

    expect(requestImage).not.toHaveBeenCalled();
    expect(container.querySelectorAll("img")).toHaveLength(0);
    expect(screen.getAllByTestId("file-preview-rejected-image")).toHaveLength(3);
    expect(container.innerHTML).not.toContain("tracker.example");
  });

  it("stops requesting images once the child-handle budget is exhausted", () => {
    const requestImage = vi.fn(async () => childAsset());
    const props = markdownProps("![logo](./img/logo.png)\n", {
      markdown: capabilityOf({ requestImage, remainingChildHandles: 0 }),
    });
    render(<FilePreviewText {...props} />);

    expect(requestImage).not.toHaveBeenCalled();
    expect(screen.getByTestId("file-preview-rejected-image")).toHaveTextContent(/limit/i);
  });

  it("reports a failed image request inline without mounting a source", async () => {
    const requestImage = vi.fn(async () => {
      throw { code: "INVALID_PATH", message: "no such file", details: { reason: "MissingFile" } };
    });
    const props = markdownProps("![logo](./img/logo.png)\n", {
      markdown: capabilityOf({ requestImage }),
    });
    const { container } = render(<FilePreviewText {...props} />);

    const failure = await screen.findByTestId("file-preview-image-error");
    expect(failure).toHaveTextContent("MissingFile");
    expect(container.querySelectorAll("img")).toHaveLength(0);
  });

  it("ignores a late image resolution from a superseded generation", async () => {
    const pending = deferred<FilePreviewChildAsset>();
    const props = markdownProps("![logo](./img/logo.png)\n", {
      markdown: capabilityOf({ requestImage: vi.fn(() => pending.promise) }),
    });
    const { rerender, container } = render(<FilePreviewText {...props} />);

    const next = markdownProps("plain text only\n", { generation: 2 });
    rerender(<FilePreviewText {...next} />);
    pending.resolve(childAsset({ mediaUrl: "http://127.0.0.1:53211/asset/stale" }));
    await Promise.resolve();

    expect(container.innerHTML).not.toContain("asset/stale");
    expect(container.querySelectorAll("img")).toHaveLength(0);
  });
});

describe("FilePreviewText Markdown source toggle", () => {
  it("switches between rendered document and read-only source", () => {
    const props = markdownProps("# Title\n\n| a |\n| --- |\n| 1 |\n");
    const { rerender } = render(<FilePreviewText {...props} />);

    expect(screen.getByTestId("file-preview-markdown")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("file-preview-source-toggle"));
    expect(props.onSourceModeChange).toHaveBeenCalledWith(true);

    rerender(<FilePreviewText {...props} sourceMode />);
    expect(screen.queryByTestId("file-preview-markdown")).toBeNull();
    expect(screen.getByTestId("file-preview-source")).toBeInTheDocument();
    expect(screen.getByTestId("file-preview-source").querySelector("table")).toBeNull();
    expect(screen.getByText("| a |")).toBeInTheDocument();
  });

  it("routes a Markdown line target into the source view instead of an editable caret", () => {
    const props = markdownProps("# Title\n\nbody\n", { target: { line: 3, col: 2 } });
    const { rerender } = render(<FilePreviewText {...props} />);

    fireEvent.click(screen.getByTestId("file-preview-target-source"));
    expect(props.onSourceModeChange).toHaveBeenCalledWith(true);

    rerender(<FilePreviewText {...props} sourceMode />);
    expect(screen.getByTestId("file-preview-location")).toHaveTextContent("Ln 3, Col 2");
    expect(screen.getByTestId("file-preview-caret")).toHaveAttribute("data-offset", "1");
  });
});
