import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  DAEMON_CONNECTION_COPY,
  DaemonConnectionBanner,
  type ProjectConnectionError,
} from "./DaemonConnectionBanner";

function daemonConnectionErrorCodes(): string[] {
  const source = readFileSync(resolve(process.cwd(), "../src-tauri/src/ipc/error.rs"), "utf8");
  const body = source.slice(source.indexOf("pub enum IpcErrorCode {"));
  const variants = body.slice(0, body.indexOf("\n}")).match(/^\s{4}([A-Z][A-Za-z0-9]*),/gm) ?? [];
  return variants
    .map((line) => line.trim().replace(/,$/, ""))
    .filter((name) => name.startsWith("Daemon") || name === "HostUnavailable")
    .map((name) => name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase());
}

describe("DaemonConnectionBanner", () => {
  afterEach(() => {
    cleanup();
  });

  it("maps every daemon-client connection error code the backend can emit", () => {
    const codes = daemonConnectionErrorCodes();
    expect(codes).toEqual(expect.arrayContaining([
      "DAEMON_BUSY", "HOST_UNAVAILABLE", "DAEMON_RETIRING", "DAEMON_DETACH_UNAVAILABLE",
    ]));
    for (const code of codes) {
      expect(DAEMON_CONNECTION_COPY[code], code).toBeDefined();
    }
    // Never emitted by the backend; a mapping for it would hide unmapped real codes.
    expect(DAEMON_CONNECTION_COPY.DAEMON_UNAVAILABLE).toBeUndefined();
  });

  it.each(["DAEMON_BUSY", "HOST_UNAVAILABLE", "DAEMON_RETIRING", "DAEMON_DETACH_UNAVAILABLE"])(
    "renders specific copy for %s instead of the raw message",
    (code) => {
      render(<DaemonConnectionBanner error={{ code, message: "raw backend text" }} onRetry={vi.fn()} />);

      expect(screen.getByText(DAEMON_CONNECTION_COPY[code].title)).toBeInTheDocument();
      expect(screen.getByText(DAEMON_CONNECTION_COPY[code].body)).toBeInTheDocument();
      expect(screen.queryByText("raw backend text")).toBeNull();
      expect(screen.getByText(code)).toBeInTheDocument();
    },
  );

  it("renders HOST_UNAVAILABLE error correctly", () => {
    const error: ProjectConnectionError = {
      code: "HOST_UNAVAILABLE",
      message: "Socket connection refused",
    };
    const onRetry = vi.fn();

    render(<DaemonConnectionBanner error={error} onRetry={onRetry} />);

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByTestId("daemon-connection-banner")).toBeInTheDocument();
    expect(
      screen.getByText("Ferryx can't reach its background service")
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Your saved tabs and panes are kept. Retry, or quit and reopen Ferryx if this keeps happening."
      )
    ).toBeInTheDocument();
    expect(screen.getByText("HOST_UNAVAILABLE")).toBeInTheDocument();

    const retryBtn = screen.getByTestId("daemon-connection-retry");
    expect(retryBtn).toBeInTheDocument();
    fireEvent.click(retryBtn);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("renders DAEMON_PROTOCOL_MISMATCH error correctly", () => {
    const error: ProjectConnectionError = {
      code: "DAEMON_PROTOCOL_MISMATCH",
      message: "Protocol version 4 does not match expected 5",
    };
    const onRetry = vi.fn();

    render(<DaemonConnectionBanner error={error} onRetry={onRetry} />);

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The background service is from a different Ferryx version"
      )
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Running terminals are kept. Quit and reopen Ferryx to finish the update."
      )
    ).toBeInTheDocument();
    expect(screen.getByText("DAEMON_PROTOCOL_MISMATCH")).toBeInTheDocument();
  });

  it("renders generic/other error code with error.message as body", () => {
    const error: ProjectConnectionError = {
      code: "PROJECT_NOT_FOUND",
      message: "The requested project path does not exist on disk.",
    };
    const onRetry = vi.fn();

    render(<DaemonConnectionBanner error={error} onRetry={onRetry} />);

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("Couldn't open this project")).toBeInTheDocument();
    expect(
      screen.getByText("The requested project path does not exist on disk.")
    ).toBeInTheDocument();
    expect(screen.getByText("PROJECT_NOT_FOUND")).toBeInTheDocument();

    const retryBtn = screen.getByTestId("daemon-connection-retry");
    fireEvent.click(retryBtn);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
