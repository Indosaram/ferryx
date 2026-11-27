import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ApprovalActionCard } from "../MobileChatComponents";
import { DiffStatLabel, diffStatLabelText, formatCompactDiffCount } from "./diffStatLabel";

afterEach(cleanup);

describe("DiffStatLabel", () => {
  it("compacts large counts the way the reference does", () => {
    expect(formatCompactDiffCount(12)).toBe("12");
    expect(formatCompactDiffCount(1500)).toBe("1.5k");
    expect(formatCompactDiffCount(12000)).toBe("12k");
    expect(formatCompactDiffCount(2_500_000)).toBe("2.5m");
  });

  it("reads one sentence for a screen reader and keeps the halves out of it", () => {
    render(<DiffStatLabel additions={12} deletions={1} />);
    const group = screen.getByTestId("diff-stat-label");
    expect(group).toHaveAttribute("aria-label", diffStatLabelText(12, 1));
    expect(diffStatLabelText(1, 1)).toBe("1 addition, 1 deletion");
    expect(group).toHaveTextContent("+12");
    expect(group).toHaveTextContent("-1");
  });
});

describe("approval more options", () => {
  it("keeps approve/decline primary and puts cancel and always-allow behind the menu", () => {
    const onChooseOption = vi.fn();
    render(
      <ApprovalActionCard
        description="Run the migration?"
        onAccept={() => {}}
        onDecline={() => {}}
        onChooseOption={onChooseOption}
      />,
    );
    expect(screen.getByTestId("approval-accept")).toBeInTheDocument();
    expect(screen.getByTestId("approval-decline")).toBeInTheDocument();
    expect(screen.queryByTestId("approval-more-options-menu")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("approval-more-options"));
    expect(screen.getByTestId("approval-option-cancel")).toBeInTheDocument();
    const always = screen.getByTestId("approval-option-acceptForSession");
    expect(always).toHaveTextContent("Always allow this session");
    // The warning is both the tooltip text and what a screen reader hears.
    expect(always).toHaveAttribute("title");
    expect(always).toHaveAttribute("aria-description", always.getAttribute("title"));

    fireEvent.click(always);
    expect(onChooseOption).toHaveBeenCalledWith("acceptForSession", expect.anything());
    expect(screen.queryByTestId("approval-more-options-menu")).not.toBeInTheDocument();
  });

  it("closes the menu on Escape and stays absent without a handler", () => {
    render(
      <ApprovalActionCard description="x" onAccept={() => {}} onDecline={() => {}} onChooseOption={() => {}} />,
    );
    fireEvent.click(screen.getByTestId("approval-more-options"));
    expect(screen.getByTestId("approval-more-options-menu")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("approval-more-options-menu")).not.toBeInTheDocument();
    cleanup();

    render(<ApprovalActionCard description="x" onAccept={() => {}} onDecline={() => {}} />);
    expect(screen.queryByTestId("approval-more-options")).not.toBeInTheDocument();
  });

  it("honours a provider's own option list", () => {
    const onChooseOption = vi.fn();
    render(
      <ApprovalActionCard
        description="x"
        onAccept={() => {}}
        onDecline={() => {}}
        extraOptions={[{ decision: "cancel", label: "Cancel" }]}
        onChooseOption={onChooseOption}
      />,
    );
    fireEvent.click(screen.getByTestId("approval-more-options"));
    expect(screen.queryByTestId("approval-option-acceptForSession")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("approval-option-cancel"));
    expect(onChooseOption).toHaveBeenCalledWith("cancel", expect.anything());
  });
});
