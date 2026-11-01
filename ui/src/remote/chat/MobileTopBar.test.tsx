/**
 * Compact top bar contract (plan task 3).
 *
 * The bar carries the selectors the remote app and the herdr harness resolve by testid and
 * accessible name, so this suite pins each owned selector, the presence rules (attention only
 * when given, the right island gone under hideControls), the per-state status dot, the exact
 * context label, and that a click on the already-active view button still reports the mode.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MobileTopBar, type MobileTopBarProps } from "./MobileTopBar";

const baseProps: MobileTopBarProps = {
  title: "Codex",
  contextLabel: "ferryx-ui / feat-x",
  pickerOpen: false,
  onTogglePicker: vi.fn(),
  viewMode: "chat",
  onViewModeChange: vi.fn(),
};

function renderTopBar(props: Partial<MobileTopBarProps> = {}) {
  return render(<MobileTopBar {...baseProps} {...props} />);
}

describe("MobileTopBar", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders the header with the compact chat tokens and the 56px row", () => {
    renderTopBar();
    const header = screen.getByTestId("mobile-top-bar");
    expect(header).toHaveClass(
      "bg-chat-header",
      "border-b",
      "border-chat-header-border",
      "pt-safe-header"
    );
    const row = header.firstElementChild as HTMLElement;
    expect(row).toHaveClass("h-14", "flex", "items-center");
  });

  it("toggles the workspace context from the left island", () => {
    const onTogglePicker = vi.fn();
    const { rerender } = renderTopBar({ pickerOpen: false, onTogglePicker });
    const toggle = screen.getByRole("button", { name: "Change workspace context" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(onTogglePicker).toHaveBeenCalledTimes(1);
    rerender(<MobileTopBar {...baseProps} pickerOpen={true} onTogglePicker={onTogglePicker} />);
    expect(
      screen.getByRole("button", { name: "Change workspace context" })
    ).toHaveAttribute("aria-expanded", "true");
  });

  it("draws the three-bar menu glyph at 16/16/8 by 1.5px", () => {
    renderTopBar();
    const toggle = screen.getByRole("button", { name: "Change workspace context" });
    const glyph = toggle.querySelector("span");
    expect(glyph).toHaveClass("gap-1");
    const bars = toggle.querySelectorAll("span > span");
    expect(bars).toHaveLength(3);
    expect(bars[0]).toHaveClass("w-4", "h-0.5");
    expect(bars[1]).toHaveClass("w-4", "h-0.5");
    expect(bars[2]).toHaveClass("w-2", "h-0.5");
  });

  it("renders the agent logo, monochrome only for the adaptive brands", () => {
    const { container, rerender } = renderTopBar({ agentType: "claude" });
    const img = container.querySelector("img") as HTMLImageElement;
    expect(img.getAttribute("data-agent-type")).toBe("claude");
    expect(img.getAttribute("src")).toBeTruthy();
    expect(img).not.toHaveClass("agent-tab-logo--monochrome");
    rerender(<MobileTopBar {...baseProps} agentType="opencode" />);
    const opencode = container.querySelector("img") as HTMLImageElement;
    expect(opencode).toHaveClass("agent-tab-logo--monochrome");
    rerender(<MobileTopBar {...baseProps} agentType="gemini" />);
    expect(container.querySelector("img")).toBeNull();
    rerender(<MobileTopBar {...baseProps} agentType={null} />);
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders the title with the compact chat typography", () => {
    renderTopBar({ title: "Refactor lanes" });
    const title = screen.getByText("Refactor lanes");
    expect(title).toHaveClass("min-w-12", "truncate", "text-base", "font-medium", "text-chat-foreground");
    expect(title.parentElement?.parentElement).toHaveClass("flex-1", "min-w-12");
  });

  it("shows a pulsing working dot with its screen-reader label", () => {
    const { container } = renderTopBar({ activityState: "working" });
    const dot = container.querySelector(".bg-status-working");
    expect(dot).not.toBeNull();
    expect(dot).toHaveClass("size-1.5", "rounded-full", "motion-safe:animate-pulse");
    expect(screen.getByText("Working")).toHaveClass("sr-only");
  });

  it("shows a steady warning dot with its screen-reader label while waiting", () => {
    const { container } = renderTopBar({ activityState: "waiting" });
    const dot = container.querySelector(".bg-status-warning");
    expect(dot).not.toBeNull();
    expect(dot).toHaveClass("size-1.5", "rounded-full");
    expect(dot).not.toHaveClass("motion-safe:animate-pulse");
    expect(screen.getByText("Needs approval")).toHaveClass("sr-only");
  });

  it("shows no status dot for done or a null activity state", () => {
    const { container, rerender } = renderTopBar({ activityState: "done" });
    expect(container.querySelector(".bg-status-working")).toBeNull();
    expect(container.querySelector(".bg-status-warning")).toBeNull();
    expect(screen.queryByText("Working")).not.toBeInTheDocument();
    expect(screen.queryByText("Needs approval")).not.toBeInTheDocument();
    rerender(<MobileTopBar {...baseProps} activityState={null} />);
    expect(container.querySelector(".bg-status-working")).toBeNull();
    expect(container.querySelector(".bg-status-warning")).toBeNull();
  });

  it("renders the exact context label on its own subtitle span with a min-width floor", () => {
    renderTopBar({ contextLabel: "ferryx-ui / feat-x" });
    const context = screen.getByLabelText("Current desktop context");
    expect(context.textContent).toBe("ferryx-ui / feat-x");
    expect(context).toHaveClass("min-w-8", "shrink", "truncate");
    expect(context).not.toHaveClass("min-w-0");
    expect(context.parentElement).toHaveClass("text-[0.8125rem]", "text-chat-foreground-secondary");
    expect(context).not.toHaveAttribute("role");
  });

  it("names the context and connection through sr-only text, not aria-label on generic spans", () => {
    renderTopBar({
      contextLabel: "ferryx-ui / feat-x",
      hostBadge: { label: "maho-win", transport: "relay", ariaLabel: "Connection: Relay" },
    });
    const badge = screen.getByTestId("remote-connection-badge");
    expect(badge).not.toHaveAttribute("aria-label");
    expect(badge).toHaveTextContent("Connection: Relay");
    expect(screen.getByText("Connection: Relay")).toHaveClass("sr-only");

    expect(screen.getByLabelText("Current desktop context")).toBe(
      screen.getByTestId("remote-context-label"),
    );
    expect(screen.queryByRole("group", { name: "Current desktop context" })).toBeNull();
    expect(screen.getByTestId("mobile-top-bar").querySelector("span[aria-label]:not([role])")).toBeNull();
  });

  it("holds the connection badge with its transport, glyph and yielding label", () => {
    renderTopBar({
      hostBadge: { label: "maho-win", transport: "relay", ariaLabel: "Connection: Relay" },
    });
    const badge = screen.getByTestId("remote-connection-badge");
    expect(badge).toHaveAttribute("data-connection", "relay");
    expect(badge).toHaveTextContent("Connection: Relay");
    expect(badge).toHaveTextContent("maho-win");
    expect(badge.querySelector("svg")).toBeInTheDocument();
    expect(badge).toHaveClass("max-w-[50%]", "shrink-0");
    expect(badge).not.toHaveClass("shrink");
    expect(badge).not.toHaveClass("min-w-[4ch]");
    expect(badge).not.toHaveClass("shrink-[100]");
    expect(badge.lastElementChild).toHaveClass("min-w-0", "truncate");
    expect(badge.lastElementChild).not.toHaveClass("max-w-[40%]", "shrink-[100]");
    expect(badge.lastElementChild).toHaveTextContent("maho-win");
    const separator = screen.getByText("·");
    expect(separator).toHaveClass("text-chat-foreground-tertiary");
  });

  it("omits the connection badge and its separator when no host badge is given", () => {
    renderTopBar();
    expect(screen.queryByTestId("remote-connection-badge")).not.toBeInTheDocument();
    expect(screen.queryByText("·")).not.toBeInTheDocument();
  });

  it("omits the attention badge unless the attention prop is given", () => {
    renderTopBar();
    expect(screen.queryByTestId("remote-attention-badge")).not.toBeInTheDocument();
  });

  it("renders the attention badge with label, count and disabled state", () => {
    renderTopBar({
      attention: {
        label: "Fix tests",
        count: 3,
        ariaLabel: "Fix tests (3 waiting)",
        disabled: true,
        onPress: vi.fn(),
      },
    });
    const badge = screen.getByTestId("remote-attention-badge");
    expect(badge).toHaveAttribute("aria-label", "Fix tests (3 waiting)");
    expect(badge).toBeDisabled();
    expect(badge).toHaveTextContent("Fix tests");
    expect(badge).toHaveTextContent("3");
    expect(badge).toHaveClass("h-11", "text-status-warning", "text-xs", "min-w-0");
    expect(badge).not.toHaveClass("shrink-0");
    expect(badge.parentElement).toHaveClass("min-w-0");
    expect(badge.lastElementChild).toHaveClass("shrink-0");
  });

  it("fires onPress for an enabled attention badge and hides the count at one", () => {
    const onPress = vi.fn();
    renderTopBar({
      attention: {
        label: "Fix tests",
        count: 1,
        ariaLabel: "Fix tests (waiting)",
        disabled: false,
        onPress,
      },
    });
    const badge = screen.getByTestId("remote-attention-badge");
    expect(badge).not.toBeDisabled();
    expect(badge.textContent).not.toMatch(/\b1\b/);
    fireEvent.click(badge);
    expect(onPress).toHaveBeenCalledTimes(1);
  });

  it("keeps both view buttons mounted and chat pressed in chat mode", () => {
    renderTopBar({ viewMode: "chat" });
    const chat = screen.getByTestId("remote-view-mode-chat");
    const terminal = screen.getByTestId("remote-view-mode-terminal");
    expect(chat).toHaveAttribute("aria-pressed", "true");
    expect(terminal).toHaveAttribute("aria-pressed", "false");
    expect(chat).toHaveClass("bg-chat-surface-hover", "text-chat-foreground");
    expect(terminal).toHaveClass("text-chat-foreground-secondary");
    expect(chat).toHaveAccessibleName("Chat");
    expect(terminal).toHaveAccessibleName("Terminal");
    expect(screen.getByRole("group", { name: "View" })).toBeInTheDocument();
  });

  it("keeps both view buttons mounted and terminal pressed in terminal mode", () => {
    renderTopBar({ viewMode: "terminal" });
    expect(screen.getByTestId("remote-view-mode-chat")).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByTestId("remote-view-mode-terminal")).toHaveAttribute("aria-pressed", "true");
  });

  it("reports the same value when the already-active view button is clicked", () => {
    const onViewModeChange = vi.fn();
    renderTopBar({ viewMode: "chat", onViewModeChange });
    fireEvent.click(screen.getByTestId("remote-view-mode-chat"));
    expect(onViewModeChange).toHaveBeenCalledTimes(1);
    expect(onViewModeChange).toHaveBeenCalledWith("chat");
  });

  it("reports terminal when the terminal view button is clicked", () => {
    const onViewModeChange = vi.fn();
    renderTopBar({ viewMode: "chat", onViewModeChange });
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));
    expect(onViewModeChange).toHaveBeenCalledWith("terminal");
  });

  it("renders the sessions button with its count badge when sessionCount is defined", () => {
    const onOpenSwitcher = vi.fn();
    renderTopBar({ sessionCount: 2, switcherOpen: true, onOpenSwitcher });
    const button = screen.getByRole("button", { name: "Sessions (2)" });
    expect(button).toHaveAttribute("aria-haspopup", "dialog");
    expect(button).toHaveAttribute("aria-expanded", "true");
    const badge = button.querySelector("span") as HTMLElement;
    expect(badge).toHaveTextContent("2");
    expect(badge).toHaveClass(
      "min-w-5",
      "h-5",
      "rounded",
      "border",
      "text-xs",
      "font-medium",
      "tabular-nums"
    );
    fireEvent.click(button);
    expect(onOpenSwitcher).toHaveBeenCalledTimes(1);
  });

  it("omits the sessions button without a sessionCount", () => {
    renderTopBar();
    expect(screen.queryByRole("button", { name: /Sessions \(/ })).not.toBeInTheDocument();
  });

  it("renders the overflow button wired to onOpenMenu", () => {
    const onOpenMenu = vi.fn();
    renderTopBar({ menuOpen: false, onOpenMenu });
    const button = screen.getByRole("button", { name: "More actions" });
    expect(button).toHaveAttribute("aria-haspopup", "dialog");
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(button.querySelector("svg")).toBeInTheDocument();
    fireEvent.click(button);
    expect(onOpenMenu).toHaveBeenCalledTimes(1);
  });

  it("removes the whole right island when hideControls is set", () => {
    renderTopBar({
      hideControls: true,
      attention: {
        label: "Fix tests",
        count: 3,
        ariaLabel: "Fix tests (3 waiting)",
        disabled: false,
        onPress: vi.fn(),
      },
      sessionCount: 3,
    });
    expect(screen.queryByTestId("remote-attention-badge")).not.toBeInTheDocument();
    expect(screen.queryByTestId("remote-view-mode-chat")).not.toBeInTheDocument();
    expect(screen.queryByTestId("remote-view-mode-terminal")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Sessions (3)" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "More actions" })).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Change workspace context" })
    ).toBeInTheDocument();
  });

  it("gives every control a 44x44 hit area in both axes", () => {
    renderTopBar({ sessionCount: 3 });
    const buttons = [
      screen.getByRole("button", { name: "Change workspace context" }),
      screen.getByTestId("remote-view-mode-chat"),
      screen.getByTestId("remote-view-mode-terminal"),
      screen.getByRole("button", { name: "Sessions (3)" }),
      screen.getByRole("button", { name: "More actions" }),
    ];
    for (const button of buttons) {
      expect(button).toHaveClass("size-11");
    }
  });

  it("provides title tooltips matching accessible names on icon-only buttons", () => {
    renderTopBar({
      sessionCount: 2,
      attention: {
        label: "Approval",
        count: 1,
        ariaLabel: "Needs approval (1)",
        disabled: false,
        onPress: vi.fn(),
      },
    });

    expect(
      screen.getByRole("button", { name: "Change workspace context" })
    ).toHaveAttribute("title", "Change workspace context");

    expect(
      screen.getByRole("button", { name: "Needs approval (1)" })
    ).toHaveAttribute("title", "Needs approval (1)");

    expect(
      screen.getByRole("button", { name: "Sessions (2)" })
    ).toHaveAttribute("title", "Sessions (2)");

    expect(screen.getByTestId("remote-view-mode-chat")).toHaveAttribute("title", "Chat");
    expect(screen.getByTestId("remote-view-mode-terminal")).toHaveAttribute("title", "Terminal");
    expect(screen.getByRole("button", { name: "More actions" })).toHaveAttribute("title", "More actions");
  });
});

