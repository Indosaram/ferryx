import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { SshAuthPromptCard } from "./SshAuthPromptCard";
import type { SshAuthPrompt } from "../../lib/sshHosts";

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

type PromptHandler = (event: { payload: SshAuthPrompt }) => void;

let handlers: PromptHandler[] = [];
const unlisten = vi.fn();

beforeEach(() => {
  handlers = [];
  unlisten.mockReset();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(undefined);
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockImplementation((async (event: string, handler: PromptHandler) => {
    expect(event).toBe("ssh_auth_prompt");
    handlers.push(handler);
    return () => {
      handlers = handlers.filter(entry => entry !== handler);
      unlisten();
    };
  }) as unknown as typeof listen);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function prompt(overrides: Partial<SshAuthPrompt> = {}): SshAuthPrompt {
  return {
    id: "p1",
    host: "alice@dev.example",
    prompt: "Enter passphrase for key '/home/alice/.ssh/id_ed25519': ",
    kind: "passphrase",
    expiresInMs: 60_000,
    ...overrides,
  };
}

function emit(payload: SshAuthPrompt) {
  act(() => {
    for (const handler of handlers) handler({ payload });
  });
}

function invokeCalls(command: string) {
  return vi.mocked(invoke).mock.calls.filter(([name]) => name === command);
}

/** Renders the card and flushes the async subscription (listen + listener registration). */
async function renderSubscribed() {
  const view = render(<SshAuthPromptCard />);
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("cmd_ssh_auth_prompt_listeners", { active: true }),
  );
  expect(handlers).toHaveLength(1);
  return view;
}

describe("SshAuthPromptCard", () => {
  it("registers as a prompt listener on mount and unregisters on unmount", async () => {
    const { unmount, container } = await renderSubscribed();
    expect(invoke).toHaveBeenCalledWith("cmd_ssh_auth_prompt_listeners", { active: true });
    expect(container).toBeEmptyDOMElement();

    unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_ssh_auth_prompt_listeners", { active: false }),
    );
  });

  it("shows a passphrase prompt and sends the typed answer", async () => {
    await renderSubscribed();
    emit(prompt());

    expect(await screen.findByText("SSH needs your input")).toBeInTheDocument();
    expect(screen.getByText("alice@dev.example")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-auth-prompt-text").textContent).toBe(
      "Enter passphrase for key '/home/alice/.ssh/id_ed25519': ",
    );
    const input = screen.getByLabelText("Key passphrase");
    expect(input).toHaveAttribute("type", "password");
    expect(input).toHaveFocus();

    const submit = screen.getByRole("button", { name: "Submit" });
    expect(submit).toBeDisabled();
    fireEvent.change(input, { target: { value: "s3cret" } });
    expect(submit).toBeEnabled();
    fireEvent.click(submit);

    await waitFor(() => expect(screen.queryByTestId("ssh-auth-prompt")).toBeNull());
    expect(invoke).toHaveBeenCalledWith("cmd_ssh_answer_auth_prompt", { id: "p1", answer: "s3cret" });
  });

  it("uses a one-time-code text input for verification codes", async () => {
    await renderSubscribed();
    emit(prompt({ id: "c1", kind: "code", prompt: "(alice@bastion) Verification code: " }));

    const input = await screen.findByLabelText("Verification code");
    expect(input).toHaveAttribute("type", "text");
    expect(input).toHaveAttribute("autocomplete", "one-time-code");
    expect(input).toHaveAttribute("inputmode", "numeric");
  });

  it("sends a null answer on Cancel", async () => {
    await renderSubscribed();
    emit(prompt({ id: "x1", kind: "password", prompt: "alice@dev.example's password: " }));

    expect(await screen.findByLabelText("Password")).toHaveAttribute("type", "password");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(screen.queryByTestId("ssh-auth-prompt")).toBeNull());
    expect(invoke).toHaveBeenCalledWith("cmd_ssh_answer_auth_prompt", { id: "x1", answer: null });
  });

  it("queues prompts and shows them one after another", async () => {
    await renderSubscribed();
    emit(prompt({ id: "first", prompt: "First question: " }));
    emit(prompt({ id: "second", kind: "other", prompt: "Second question: " }));

    expect(await screen.findByText("First question:", { exact: false })).toBeInTheDocument();
    expect(screen.queryByText("Second question:", { exact: false })).toBeNull();

    fireEvent.change(screen.getByLabelText("Key passphrase"), { target: { value: "one" } });
    fireEvent.click(screen.getByRole("button", { name: "Submit" }));

    expect(await screen.findByText("Second question:", { exact: false })).toBeInTheDocument();
    expect(screen.queryByText("First question:", { exact: false })).toBeNull();
    const next = screen.getByLabelText("Response");
    expect(next).toHaveValue("");
    expect(invoke).toHaveBeenCalledWith("cmd_ssh_answer_auth_prompt", { id: "first", answer: "one" });
    expect(invokeCalls("cmd_ssh_answer_auth_prompt")).toHaveLength(1);
  });

  it("drops an expired prompt without answering it", async () => {
    await renderSubscribed();
    // Fake timers only after subscribing, so waitFor above polls with real timers.
    vi.useFakeTimers();
    emit(prompt({ id: "e1", expiresInMs: 5_000 }));

    expect(screen.getByTestId("ssh-auth-prompt")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(4_999);
    });
    expect(screen.getByTestId("ssh-auth-prompt")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByTestId("ssh-auth-prompt")).toBeNull();
    expect(invokeCalls("cmd_ssh_answer_auth_prompt")).toHaveLength(0);
  });
});
