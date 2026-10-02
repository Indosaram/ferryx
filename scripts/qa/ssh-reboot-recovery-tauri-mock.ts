import type { StructuredIpcError } from "../../ui/src/lib/types";

export type RegisteredProject = Record<string, unknown>;
export type SpawnTerminalRequest = Record<string, unknown>;
export type SpawnTerminalResult = { sessionId: string; [key: string]: unknown };
export type TerminalDescribeResult = Record<string, unknown>;

export function isStructuredIpcError(error: unknown): error is StructuredIpcError {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof (error as Record<string, unknown>).code === "string" &&
    typeof (error as Record<string, unknown>).message === "string"
  );
}

export function toIpcError(error: unknown): StructuredIpcError {
  if (isStructuredIpcError(error)) {
    return error;
  }
  if (error instanceof Error) {
    return {
      code: "UNKNOWN",
      message: error.message,
      details: {},
    };
  }
  if (typeof error === "string") {
    return {
      code: "UNKNOWN",
      message: error,
      details: {},
    };
  }
  return {
    code: "UNKNOWN",
    message: String(error),
    details: {},
  };
}

export async function retryTerminalRemoteSession(_sessionId: string): Promise<{ type: "retryRemoteSessionOk" }> {
  return { type: "retryRemoteSessionOk" };
}

export async function scrollNativeTerminal(
  _sessionId: string,
  _behavior: unknown,
  _options?: unknown,
): Promise<void> {
  return Promise.resolve();
}

export async function describeTerminal(_sessionId: string): Promise<TerminalDescribeResult | null> {
  return null;
}

export async function getTerminalHistorySnapshot(_sessionId: string): Promise<string> {
  return "";
}

export async function onNativeTerminalAgentState(
  _handler: (payload: unknown) => void,
): Promise<() => void> {
  return () => undefined;
}

export async function spawnTerminalDetailed(
  _request: unknown,
  _options?: unknown,
): Promise<never> {
  throw new Error("UNEXPECTED_MUTATION: spawnTerminalDetailed called unexpectedly in isolated recovery harness");
}

export async function closeTerminal(_sessionId: string): Promise<never> {
  throw new Error("UNEXPECTED_MUTATION: closeTerminal called unexpectedly in isolated recovery harness");
}

export async function suspendTerminal(_sessionId: string): Promise<never> {
  throw new Error("UNEXPECTED_MUTATION: suspendTerminal called unexpectedly in isolated recovery harness");
}

export async function resumeTerminal(_sessionId: string): Promise<never> {
  throw new Error("UNEXPECTED_MUTATION: resumeTerminal called unexpectedly in isolated recovery harness");
}

export async function hibernateTerminal(_sessionId: string): Promise<never> {
  throw new Error("UNEXPECTED_MUTATION: hibernateTerminal called unexpectedly in isolated recovery harness");
}
