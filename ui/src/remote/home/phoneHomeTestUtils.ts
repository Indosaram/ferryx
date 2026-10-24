// Test helper: open the worktree picker on whichever phone surface is showing. On the phone
// home (no session open yet) the picker is the Worktrees tab's inline inventory; on the Session
// screen it is the "Change workspace context" drawer, exactly as before the home existed.
import { act, fireEvent, screen, waitFor } from "@testing-library/react";

export async function openWorkspacePicker(options: { timeout?: number } = {}): Promise<HTMLElement> {
  const trigger = await waitFor(
    () => {
      const found =
        screen.queryByTestId("mobile-home-tab-worktrees") ??
        screen.queryByRole("button", { name: /Change workspace context/i });
      if (!found) throw new Error("Neither the Worktrees tab nor the workspace context trigger is rendered");
      return found;
    },
    options.timeout === undefined ? undefined : { timeout: options.timeout },
  );
  const onHome = trigger.getAttribute("data-testid") === "mobile-home-tab-worktrees";
  if (onHome || trigger.getAttribute("aria-expanded") !== "true") {
    act(() => {
      fireEvent.click(trigger);
    });
  }
  return waitFor(() => {
    const picker = onHome
      ? screen.queryByTestId("remote-worktrees-inline")
      : (screen.queryByRole("dialog", { name: "Workspace context" }) ?? screen.queryByTestId("remote-desktop-sidebar"));
    if (!picker) throw new Error("The worktree picker did not open");
    return picker;
  });
}
