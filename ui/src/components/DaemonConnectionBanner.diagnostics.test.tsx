import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { redactSecrets } from "../lib/daemonDiagnostics";
import {
  DaemonConnectionBanner,
  type ProjectConnectionError,
} from "./DaemonConnectionBanner";

const INSTALL_KEY = `fxk_${"0123456789abcdef".repeat(4)}`;
const BEARER = "Bearer abc.def.ghi1234567890abcdefghij";

function connectFailure(): ProjectConnectionError {
  return {
    code: "INTERNAL_ERROR",
    message: `Daemon process stdout closed before emitting readiness signal token=supersecretvalue123`,
    details: {
      type: "daemonConnectFailure",
      diagnostics: {
        stage: "readinessClosed",
        reason: "codesign --verify failed for the staged runtime",
        socketPath: "/tmp/ferryx-1000/daemon.sock",
        logPath: "/home/u/.ferryx/logs/daemon.log",
        stderrTail: `panic: FERRYX_INSTALL_KEY=${INSTALL_KEY} ${BEARER}`,
        ownership: { problem: null, expectedUid: 1000 },
      },
    },
  };
}

describe("DaemonConnectionBanner diagnostics", () => {
  const original = Object.getOwnPropertyDescriptor(navigator, "clipboard");

  afterEach(() => {
    cleanup();
    if (original) {
      Object.defineProperty(navigator, "clipboard", original);
    } else {
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  it("shows the reason, socket and log file", () => {
    render(<DaemonConnectionBanner error={connectFailure()} onRetry={vi.fn()} />);

    expect(screen.getByTestId("daemon-connection-diagnostics")).toBeInTheDocument();
    expect(screen.getByTestId("daemon-diagnostics-reason")).toHaveTextContent(
      "codesign --verify failed for the staged runtime",
    );
    expect(screen.getByTestId("daemon-diagnostics-socket")).toHaveTextContent("/tmp/ferryx-1000/daemon.sock");
    expect(screen.getByTestId("daemon-diagnostics-log")).toHaveTextContent("/home/u/.ferryx/logs/daemon.log");
  });

  it("copies a redacted report", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(<DaemonConnectionBanner error={connectFailure()} onRetry={vi.fn()} />);

    fireEvent.click(screen.getByTestId("daemon-connection-copy-diagnostics"));

    expect(await screen.findByText("Copied")).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledTimes(1);
    const copied = String(writeText.mock.calls[0]?.[0]);
    expect(copied).toContain("code: INTERNAL_ERROR");
    expect(copied).toContain("/tmp/ferryx-1000/daemon.sock");
    expect(copied).toContain("/home/u/.ferryx/logs/daemon.log");
    expect(copied).toContain("readinessClosed");
    expect(copied).not.toContain(INSTALL_KEY);
    expect(copied).not.toContain("abc.def.ghi1234567890abcdefghij");
    expect(copied).not.toContain("supersecretvalue123");
  });

  it("reports a failed copy", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(<DaemonConnectionBanner error={connectFailure()} onRetry={vi.fn()} />);

    fireEvent.click(screen.getByTestId("daemon-connection-copy-diagnostics"));

    expect(await screen.findByText("Copy failed")).toBeInTheDocument();
  });

  it("shows no diagnostics or copy action without details", () => {
    render(
      <DaemonConnectionBanner
        error={{ code: "DAEMON_UNAVAILABLE", message: "Socket connection refused" }}
        onRetry={vi.fn()}
      />,
    );

    expect(screen.queryByTestId("daemon-connection-diagnostics")).toBeNull();
    expect(screen.queryByTestId("daemon-connection-copy-diagnostics")).toBeNull();
    expect(screen.getByTestId("daemon-connection-retry")).toBeInTheDocument();
  });
});

describe("redactSecrets", () => {
  it.each([
    [`key ${INSTALL_KEY} rejected`, INSTALL_KEY],
    [BEARER, "abc.def.ghi1234567890abcdefghij"],
    ["Authorization: Basic dXNlcjpwYXNz", "dXNlcjpwYXNz"],
    ['{"token":"s3cr3t-value"}', "s3cr3t-value"],
    ["password=hunter2 next", "hunter2"],
    ["api_key: K9zz", "K9zz"],
    ["-----BEGIN OPENSSH PRIVATE KEY-----\nAAAAkey\n-----END OPENSSH PRIVATE KEY-----", "AAAAkey"],
  ])("redacts %s", (input, secret) => {
    const redacted = redactSecrets(input);
    expect(redacted).not.toContain(secret);
    expect(redacted).toContain("[redacted]");
  });

  it("keeps a JSON pair valid and leaves paths alone", () => {
    expect(redactSecrets('{"token":"x1"}')).toBe('{"token":"[redacted]"}');
    for (const plain of [
      "/tmp/ferryx-1000/daemon.sock",
      "/home/u/.ferryx/logs/daemon.log",
      "Path /tmp/ferryx-1000 is owned by UID 0 (expected current UID 1000)",
    ]) {
      expect(redactSecrets(plain)).toBe(plain);
    }
  });
});
