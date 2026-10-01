import { describe, it, expect, afterEach, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { PlanLimitNotice, DEFAULT_PRICING_URL } from "./PlanLimitNotice";
import { PLAN_LIMIT_REACHED, REMOTE_SUSPENDED } from "./accountSession";

describe("PlanLimitNotice", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders the plan-limit state with usage numbers and the pricing link", () => {
    render(
      <PlanLimitNotice state={{ code: PLAN_LIMIT_REACHED, plan: "free", limit: 1, used: 2 }} />,
    );

    const notice = screen.getByTestId("remote-plan-limit-notice");
    expect(notice.getAttribute("data-code")).toBe(PLAN_LIMIT_REACHED);
    expect(notice.getAttribute("data-plan")).toBe("free");
    expect(notice.getAttribute("role")).toBe("alert");

    const usage = screen.getByTestId("remote-plan-limit-usage");
    expect(usage.getAttribute("data-limit")).toBe("1");
    expect(usage.getAttribute("data-used")).toBe("2");

    const upgrade = screen.getByTestId("remote-plan-limit-upgrade");
    expect(upgrade.getAttribute("href")).toBe(DEFAULT_PRICING_URL);
    expect(upgrade.getAttribute("target")).toBe("_blank");

    expect(screen.getByTestId("remote-plan-limit-actions").className).toContain("flex-wrap");
    expect(screen.queryByTestId("remote-plan-limit-grace-ends")).toBeNull();
    expect(screen.queryByTestId("remote-plan-limit-stopped")).toBeNull();
  });

  it("renders suspension deadlines as machine-readable times", () => {
    const graceEndsAt = 1_700_000_000;
    const stoppedAt = 1_700_086_400;
    render(
      <PlanLimitNotice
        state={{
          code: REMOTE_SUSPENDED,
          plan: "pro_monthly",
          status: "stopped",
          graceEndsAt,
          stoppedAt,
        }}
      />,
    );

    expect(screen.getByTestId("remote-plan-limit-notice").getAttribute("data-code")).toBe(
      REMOTE_SUSPENDED,
    );
    expect(screen.getByTestId("remote-plan-limit-grace-ends").getAttribute("datetime")).toBe(
      new Date(graceEndsAt * 1000).toISOString(),
    );
    expect(screen.getByTestId("remote-plan-limit-stopped").getAttribute("datetime")).toBe(
      new Date(stoppedAt * 1000).toISOString(),
    );
  });

  it("never fabricates timestamps or usage when only the close reason is known", () => {
    const { container } = render(<PlanLimitNotice state={{ code: REMOTE_SUSPENDED }} />);

    const notice = screen.getByTestId("remote-plan-limit-notice");
    expect(notice.getAttribute("data-code")).toBe(REMOTE_SUSPENDED);
    expect(notice.getAttribute("data-plan")).toBe("");
    expect(container.querySelectorAll("time")).toHaveLength(0);
    expect(screen.queryByTestId("remote-plan-limit-usage")).toBeNull();
    expect(screen.getByTestId("remote-plan-limit-upgrade")).toBeDefined();
    expect(screen.queryByTestId("remote-plan-limit-retry")).toBeNull();
  });

  it("never crashes on unrenderable timestamps passed directly", () => {
    const { container } = render(
      <PlanLimitNotice
        state={{ code: REMOTE_SUSPENDED, graceEndsAt: Number.MAX_VALUE, stoppedAt: Number.NaN }}
      />,
    );

    expect(screen.getByTestId("remote-plan-limit-notice")).toBeDefined();
    expect(container.querySelectorAll("time")).toHaveLength(0);
  });

  it("offers the explicit recovery retry only when a handler is provided", () => {
    const onRetry = vi.fn();
    const { rerender } = render(
      <PlanLimitNotice state={{ code: REMOTE_SUSPENDED }} onRetry={onRetry} />,
    );

    fireEvent.click(screen.getByTestId("remote-plan-limit-retry"));
    expect(onRetry).toHaveBeenCalledTimes(1);

    rerender(<PlanLimitNotice state={{ code: REMOTE_SUSPENDED }} />);
    expect(screen.queryByTestId("remote-plan-limit-retry")).toBeNull();
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
