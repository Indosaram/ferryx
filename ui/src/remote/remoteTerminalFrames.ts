export const REMOTE_TERMINAL_FRAME_PREFIX = "\x1b]777;ferryx;";
export const REMOTE_TERMINAL_FRAME_TERMINATOR = "\x07";
export const REMOTE_TERMINAL_HARD_RESET = "\x1bc";

export function decodeRemoteTerminalFrame(raw: string): { kind: string; payload: string } | null {
  if (!raw.startsWith(REMOTE_TERMINAL_FRAME_PREFIX)) return null;
  const terminatorIndex = raw.indexOf(
    REMOTE_TERMINAL_FRAME_TERMINATOR,
    REMOTE_TERMINAL_FRAME_PREFIX.length,
  );
  if (terminatorIndex < 0) return null;
  let metadata: { kind?: string };
  try {
    metadata = JSON.parse(raw.slice(REMOTE_TERMINAL_FRAME_PREFIX.length, terminatorIndex));
  } catch {
    return null;
  }
  let payload = raw.slice(terminatorIndex + REMOTE_TERMINAL_FRAME_TERMINATOR.length);
  if (payload.startsWith(REMOTE_TERMINAL_HARD_RESET)) {
    payload = payload.slice(REMOTE_TERMINAL_HARD_RESET.length);
  }
  return { kind: metadata.kind ?? "output", payload };
}

export function stripTerminalControlSequences(text: string): string {
  let out = text.replace(/\r\n/g, "\n");
  out = out.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "");
  out = out.replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "");
  out = out.replace(/\x1b[^\x1b]/g, "");
  out = out.replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g, "");
  return out
    .split("\n")
    .map((line) => {
      const lastCr = line.lastIndexOf("\r");
      return lastCr >= 0 ? line.slice(lastCr + 1) : line;
    })
    .join("\n");
}
