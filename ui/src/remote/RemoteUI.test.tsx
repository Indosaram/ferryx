import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MobileKeyDock } from "../components/MobileKeyDock";
import { PairingPage } from "./PairingPage";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ sessionId }: { sessionId: string }) => (
    <div data-testid="remote-terminal" data-session-id={sessionId}>
      Mirrored terminal {sessionId}
    </div>
  ),
}));

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function jsonResponse(body: unknown, ok = true): Response {
  return {
    ok,
    status: ok ? 200 : 500,
    json: vi.fn(async () => body),
  } as unknown as Response;
}

const focusedState = {
  activeContext: {
    workspaceId: "ferryx-ui",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTerminal: {
      sessionId: "focused-terminal",
      title: "Focused desktop terminal",
      running: true,
    },
  },
  projects: [
    {
      workspaceId: "ferryx-ui",
      repoRoot: "/Users/alice/secret/ferryx-ui",
      worktrees: [
        {
          slug: "main",
          label: "main",
          path: "/Users/alice/secret/ferryx-ui",
        },
      ],
    },
    {
      workspaceId: "api-service",
      repoRoot: "/Volumes/private/api-service",
      worktrees: [
        {
          slug: "feature/remote-safe",
          label: "feature/remote-safe",
          path: "/Volumes/private/api-service-worktree",
        },
      ],
    },
  ],
  sessions: [
    {
      sessionId: "focused-terminal",
      title: "Focused desktop terminal",
      workspaceId: "ferryx-ui",
      worktreeLabel: "main",
      running: true,
    },
    {
      sessionId: "background-terminal",
      title: "Hidden /Users/alice/secret shell",
      workspaceId: "api-service",
      worktreeLabel: "private-background",
      running: true,
    },
  ],
};

const currentNativeState = {
  activeWorkspaceId: "ferryx-ui",
  projects: [
    { workspaceId: "ferryx-ui", repoRoot: "/Users/alice/secret/ferryx-ui" },
    { workspaceId: "api-service", repoRoot: "/Volumes/private/api-service" },
  ],
  worktrees: [
    {
      path: "/Users/alice/secret/ferryx-ui",
      branch: "refs/heads/orca/ferryx-ui/main",
    },
  ],
  sessions: [
    {
      sessionId: "focused-terminal",
      workspaceId: "ferryx-ui",
      worktreeLabel: "main",
      running: true,
    },
  ],
};

const confirmedNoFocusState = {
  activeContext: {
    workspaceId: "api-service",
    worktreeSlug: "feature/remote-safe",
    worktreeLabel: "feature/remote-safe",
    activeTerminal: null,
  },
  projects: focusedState.projects,
  sessions: focusedState.sessions,
};

beforeEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe("Remote UI Components", () => {
  it("PairingPage renders the Ferryx Desktop PIN flow", () => {
    render(<PairingPage onPaired={vi.fn()} />);

    const input = screen.getByPlaceholderText(/6-digit PIN/i);
    expect(input).toHaveAttribute("maxLength", "6");
    expect(screen.getByText(/Ferryx Desktop settings/i)).toBeInTheDocument();
  });

  it("renders the native active-only state as one mirrored terminal without exposing paths", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(currentNativeState)),
    );

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "focused-terminal");
    expect(screen.getAllByTestId("remote-terminal")).toHaveLength(1);
    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "ferryx-ui / main",
    );
    expect(document.body).not.toHaveTextContent("/Users/alice/secret");
  });

  it("mirrors only the server-declared terminal and safely confirms a context selection", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockImplementationOnce(() => selectionResponse.promise)
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", fetchMock);

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "focused-terminal");
    expect(screen.getAllByTestId("remote-terminal")).toHaveLength(1);
    expect(screen.queryByText(/background-terminal|private-background|Hidden/i)).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("/Users/alice/secret");
    expect(document.body).not.toHaveTextContent("/Volumes/private");

    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "ferryx-ui / main",
    );
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));

    const selector = screen.getByRole("dialog", { name: /Workspace context/i });
    const target = within(selector).getByRole("button", {
      name: /api-service.*feature\/remote-safe/i,
    });
    fireEvent.click(target);

    expect(screen.getByRole("status")).toHaveTextContent(
      /Switching to api-service \/ feature\/remote-safe/i,
    );
    expect(target).toBeDisabled();
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "/api/v1/workspace/select?token=test-token",
      expect.objectContaining({
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          workspaceId: "api-service",
          worktreeSlug: "feature/remote-safe",
        }),
      }),
    );

    await act(async () => {
      selectionResponse.resolve(jsonResponse({ accepted: true }));
      await selectionResponse.promise;
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
        "api-service / feature/remote-safe",
      );
    });
    expect(screen.getByRole("status")).toHaveTextContent(/Desktop context confirmed/i);
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(screen.getByText("No focused terminal")).toBeInTheDocument();
    expect(
      screen.getByText(/Focus a terminal in Ferryx Desktop to mirror it here/i),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("background-terminal");
  });

  it("shows no focused terminal when only undeclared background sessions are present", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>().mockResolvedValue(
        jsonResponse({
          ...confirmedNoFocusState,
          sessions: [focusedState.sessions[1]],
        }),
      ),
    );

    render(<RemoteApp />);

    expect(await screen.findByText("No focused terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("background-terminal");
    expect(document.body).not.toHaveTextContent("Hidden /Users/alice/secret shell");
  });

  it("MobileKeyDock dispatches primary key actions and latches modifiers", () => {
    const handleSendKey = vi.fn();
    render(<MobileKeyDock onSendKey={handleSendKey} />);

    fireEvent.click(screen.getByText("Ctrl-C"));
    expect(handleSendKey).toHaveBeenCalledWith("ctrl-c");

    fireEvent.click(screen.getByText("Ctrl"));
    fireEvent.click(screen.getByText("Tab"));
    expect(handleSendKey).toHaveBeenCalledWith("ctrl-tab");
  });

  it("retains legacy authentication without rendering old Orca branding", async () => {
    localStorage.setItem("rorca_remote_token", "legacy-token");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(confirmedNoFocusState)));

    render(<RemoteApp />);

    const header = await screen.findByRole("banner");
    expect(header).toHaveTextContent("Ferryx Remote");
    expect(header.textContent?.toLowerCase()).not.toContain("orca");
  });
});
