import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { resetAppearanceSettings, saveAppearanceSettings } from "../../lib/appearanceSettings";
import { MobileChatMessage } from "./MobileChatMessage";

const KATEX_SELECTOR = ".katex";

afterEach(() => {
  cleanup();
  resetAppearanceSettings();
});

// Subscribe before rendering, rather than polling or assuming import timing.
async function renderWithMath(props: Partial<React.ComponentProps<typeof MobileChatMessage>> & { content: string }) {
  const host = document.createElement("div");
  document.body.append(host);
  let observer: MutationObserver;
  let timeout: ReturnType<typeof setTimeout>;
  const rendered = new Promise<void>((resolve, reject) => {
    observer = new MutationObserver(() => {
      if (!host.querySelector(KATEX_SELECTOR)) return;
      clearTimeout(timeout);
      observer.disconnect();
      resolve();
    });
    observer.observe(host, { childList: true, subtree: true });
    timeout = setTimeout(() => { observer.disconnect(); reject(new Error("KaTeX node was not rendered")); }, 3000);
  });
  const view = render(<MobileChatMessage id="math-test" role="assistant" {...props} />, { container: host });
  try {
    await act(async () => { await import("./chatMath"); });
    await rendered;
    return view;
  } finally {
    observer!.disconnect();
    clearTimeout(timeout!);
  }
}

describe("chat math", () => {
  it("renders inline powers through KaTeX", async () => {
    const { container } = await renderWithMath({ content: "Result: $x^2$." });
    expect(container.querySelector(".katex .katex-html .msupsub")).not.toBeNull();
    expect(container.querySelector(".katex math msup")).not.toBeNull();
  });

  it("renders a display integral with both bounds", async () => {
    const { container } = await renderWithMath({ content: String.raw`$$\int_0^1 x\,dx$$` });
    expect(container.querySelector(".katex-display .katex math msubsup")).not.toBeNull();
    expect(container.querySelector(".katex")?.textContent).toContain("∫");
  });

  it("renders grouped fractions and roots", async () => {
    const { container } = await renderWithMath({ content: String.raw`$\frac{x^2}{\sqrt{y}}$` });
    expect(container.querySelector(".katex math mfrac")).not.toBeNull();
    expect(container.querySelector(".katex math msqrt")).not.toBeNull();
  });

  it("does not transform fenced, indented, or inline code", async () => {
    const content = "$y$\n\n`$x$`\n\n```text\n$$x^2$$\n```\n\n    $z$\n";
    const { container } = await renderWithMath({ content });
    expect(container.querySelectorAll(".katex")).toHaveLength(1);
    const code = [...container.querySelectorAll("code")].map((node) => node.textContent).join("\n");
    expect(code).toContain("$x$");
    expect(code).toContain("$$x^2$$");
    expect(code).toContain("$z$");
  });

  it("shows the exact raw source when disabled, and responds to a setting change", async () => {
    const content = String.raw`**Result:** $x^2$ and $$\int_0^1 x\,dx$$`;
    const { container } = await renderWithMath({ content });
    // A store update notifies the subscribed prose even when the message's props
    // stay identical and React.memo skips rerendering the parent message.
    act(() => { saveAppearanceSettings({ renderMath: false }); });
    expect(container.querySelector(".katex, .katex-display")).toBeNull();
    expect(screen.getByTestId("assistant-message-body").textContent).toBe(content);
  });

  it("preserves unterminated expressions without throwing", () => {
    const { container } = render(<MobileChatMessage id="partial" role="assistant" content="Unfinished $x" />);
    expect(container.textContent).toContain("Unfinished $x");
    expect(container.querySelector(".katex")).toBeNull();
  });

  it("keeps an unsupported command visible instead of dropping the message", async () => {
    const { container } = await renderWithMath({ content: String.raw`Bad: $\unsupported{x}$` });
    expect(container.querySelector(".katex")).not.toBeNull();
    expect(container.textContent).toContain("Bad:");
  });

  it("leaves escaped delimiters literal", () => {
    const { container } = render(<MobileChatMessage id="escaped" role="assistant" content={String.raw`Cost: \$x\$`} />);
    expect(container.querySelector(".katex")).toBeNull();
    expect(container.textContent).toContain("Cost: $x$");
  });

  it("keeps math out of user and system messages", () => {
    const { container } = render(<>
      <MobileChatMessage id="user" role="user" content="$x^2$" />
      <MobileChatMessage id="system" role="system" content="$$x$$" />
    </>);
    expect(container.querySelector(".katex")).toBeNull();
    expect(container.textContent).toContain("$x^2$");
    expect(container.textContent).toContain("$$x$$");
  });

  it("renders math in assistant rich text parts", async () => {
    const { container } = await renderWithMath({
      content: "",
      referenceParts: [{ kind: "text", text: "$x^2$" }],
      toolCalls: [],
    });
    expect(container.querySelector("[data-testid='assistant-reference-body'] .katex")).not.toBeNull();
  });

  it("does not render math in expanded tool drawers", () => {
    const { container } = render(<MobileChatMessage id="tool" role="assistant" content="" toolCalls={[
      { toolName: "bash", command: "echo '$x$'", output: "$$x^2$$", status: "success" },
    ]} />);
    fireEvent.click(screen.getByTestId("work-row"));
    expect(container.querySelector(".katex")).toBeNull();
    expect(screen.getByTestId("work-row-output").textContent).toContain("$$x^2$$");
  });
});
