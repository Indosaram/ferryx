import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MobileChatMessage } from "../MobileChatMessage";
import {
  buildPlanPreviewMarkdown,
  planIsLong,
  proposedPlanTitle,
  stripDisplayedPlanMarkdown,
} from "./planCard";
import { HTML_RENDER_SANDBOX } from "./htmlRenderFrame";
import { isHtmlRenderPart, isPlanPart, tailLines } from "./partKinds";
import type { ReferencePart } from "../referenceTypes";

afterEach(cleanup);

const planPart = (markdown: string): ReferencePart =>
  ({ kind: "plan", markdown }) as unknown as ReferencePart;
const htmlPart = (html: string): ReferencePart =>
  ({ kind: "htmlRender", html, title: "Chart" }) as unknown as ReferencePart;

describe("plan and htmlRender parts", () => {
  it("guards the grown part kinds at runtime", () => {
    expect(isPlanPart(planPart("# P"))).toBe(true);
    expect(isPlanPart({ kind: "text", text: "x" } as unknown as ReferencePart)).toBe(false);
    expect(isHtmlRenderPart(htmlPart("<p>x</p>"))).toBe(true);
    expect(isHtmlRenderPart(planPart("# P"))).toBe(false);
  });

  it("renders a plan part as a badged card with the heading as its title", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[planPart("# Migration plan\n\n1. Move types\n2. Rewire callers")]}
      />,
    );
    const card = screen.getByTestId("proposed-plan-card");
    expect(card).toHaveTextContent("Plan");
    expect(card).toHaveTextContent("Migration plan");
    expect(card).toHaveTextContent("Rewire callers");
  });

  it("collapses a long plan and expands it on request", () => {
    const lines = Array.from({ length: 40 }, (_, i) => `Step ${i + 1}`).join("\n");
    render(
      <MobileChatMessage id="m" role="assistant" content="" referenceParts={[planPart(lines)]} />,
    );
    expect(planIsLong(lines)).toBe(true);
    expect(screen.getByTestId("proposed-plan-card")).not.toHaveTextContent("Step 30");
    fireEvent.click(screen.getByTestId("plan-expand-toggle"));
    expect(screen.getByTestId("proposed-plan-card")).toHaveTextContent("Step 30");
  });

  it("renders an htmlRender part in a script-safe, non-same-origin sandbox", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[htmlPart("<p>visual reply</p>")]}
      />,
    );
    const frame = screen.getByTestId("html-render-frame");
    const iframe = screen.getByTestId("html-render-iframe");
    expect(frame).toHaveTextContent("Chart");
    expect(iframe).toHaveAttribute("sandbox", HTML_RENDER_SANDBOX);
    expect(HTML_RENDER_SANDBOX).not.toContain("allow-same-origin");
    expect(iframe).toHaveAttribute("srcdoc", "<p>visual reply</p>");
  });

  it("renders nothing new for unknown part kinds", () => {
    const unknown = { kind: "hologram", data: "x" } as unknown as ReferencePart;
    const { container } = render(
      <MobileChatMessage id="m" role="assistant" content="" referenceParts={[unknown]} />,
    );
    expect(container.textContent).toBe("");
  });

  it("previews the first lines of a long plan", () => {
    const lines = Array.from({ length: 30 }, (_, i) => `Line ${i + 1}`).join("\n");
    const preview = buildPlanPreviewMarkdown(lines);
    expect(preview.split("\n")).toHaveLength(11);
    expect(proposedPlanTitle("# Title\nbody")).toBe("Title");
    expect(proposedPlanTitle("no heading here")).toBeNull();
  });

  it("drops the heading the card already draws as its title", () => {
    // t3code stripDisplayedPlanMarkdown: the plan must not say its name twice.
    expect(stripDisplayedPlanMarkdown("# Ship it\n\n- one\n- two")).toBe("- one\n- two");
    expect(stripDisplayedPlanMarkdown("no heading here")).toBe("no heading here");
    expect(buildPlanPreviewMarkdown("# Ship it\n- one")).toBe("- one\n…");
  });
});

describe("tool output tail", () => {
  it("keeps only the most recent lines for a long running output", () => {
    const lines = Array.from({ length: 60 }, (_, i) => `line ${i + 1}`).join("\n");
    const tail = tailLines(lines, 40);
    expect(tail.split("\n")).toHaveLength(40);
    expect(tail).toContain("line 60");
    expect(tail).not.toContain("line 1\n");
    expect(tailLines("short", 40)).toBe("short");
  });
});
