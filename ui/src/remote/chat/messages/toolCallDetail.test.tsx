import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MobileChatMessage } from "../MobileChatMessage";
import type { ReferencePart } from "../referenceTypes";

afterEach(cleanup);

const tool = (overrides: Partial<Extract<ReferencePart, { kind: "tool" }>> = {}): ReferencePart => ({
  kind: "tool",
  name: "bash",
  summary: "",
  input: "bun test",
  output: "ok",
  status: "completed",
  id: "t",
  ...overrides,
});

describe("tool call detail", () => {
  it("shows the exit code beside the opened call's output", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[{ ...tool(), exitCode: 2 } as unknown as ReferencePart]}
      />,
    );
    fireEvent.click(screen.getByTestId("work-row"));
    expect(screen.getByTestId("tool-call-exit-code")).toHaveTextContent("exit 2");
  });

  it("renders no exit code when the transcript recorded none", () => {
    render(<MobileChatMessage id="m" role="assistant" content="" referenceParts={[tool()]} />);
    fireEvent.click(screen.getByTestId("work-row"));
    expect(screen.queryByTestId("tool-call-exit-code")).not.toBeInTheDocument();
  });

  it("keeps only the recent tail of a running command's output", () => {
    const output = Array.from({ length: 80 }, (_, i) => `line ${i + 1}`).join("\n");
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[tool({ status: "running", output, id: "run" })]}
      />,
    );
    fireEvent.click(screen.getByTestId("work-row"));
    const live = screen.getByTestId("reference-tool-output");
    expect(live).toHaveAttribute("data-live", "true");
    expect(live).toHaveTextContent("line 80");
    expect(live).not.toHaveTextContent("line 1\n");
    expect(live).toHaveTextContent("still running");
  });

  it("keeps a finished call's whole output", () => {
    const output = Array.from({ length: 80 }, (_, i) => `line ${i + 1}`).join("\n");
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[tool({ status: "completed", output, id: "done" })]}
      />,
    );
    fireEvent.click(screen.getByTestId("work-row"));
    const whole = screen.getByTestId("reference-tool-output");
    expect(whole).not.toHaveAttribute("data-live");
    expect(whole).toHaveTextContent("line 1");
    expect(whole).toHaveTextContent("line 80");
  });
});
