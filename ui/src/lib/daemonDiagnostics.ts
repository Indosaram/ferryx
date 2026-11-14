// Diagnostics a failed daemon connect carries in `details.diagnostics` (todo 36). Everything
// shown or copied passes through redactSecrets, because users paste it into bug reports.

export type DaemonDiagnostics = {
  stage: string | null;
  socketPath: string | null;
  logPath: string | null;
  reason: string | null;
  supervisorError: string | null;
};

type DiagnosticsSource = {
  code: string;
  message: string;
  details?: Record<string, unknown>;
};

const REDACTED = "[redacted]";

const SECRET_KEY =
  "[A-Za-z0-9_-]*(?:token|secret|password|passwd|api[_-]?key|auth|cookie|private_key)[A-Za-z0-9_-]*";

const REDACTIONS: ReadonlyArray<[RegExp, (match: string, ...groups: string[]) => string]> = [
  [
    /-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g,
    () => REDACTED,
  ],
  [/fxk_[A-Za-z0-9]+/g, () => REDACTED],
  [/(authorization"?\s*[:=]\s*"?)[^\r\n"]+/gi, (_m, prefix) => `${prefix}${REDACTED}`],
  [/(bearer\s+)[A-Za-z0-9_.~+/=-]+/gi, (_m, prefix) => `${prefix}${REDACTED}`],
  [
    new RegExp(`("?${SECRET_KEY}"?\\s*[:=]\\s*)("[^"]*"|[^\\s,;&"']+)`, "gi"),
    (_m, prefix, value) => `${prefix}${value.startsWith('"') ? `"${REDACTED}"` : REDACTED}`,
  ],
  [
    /[A-Za-z0-9+_=-]{32,}/g,
    (run) => (/[A-Za-z]/.test(run) && /[0-9]/.test(run) ? REDACTED : run),
  ],
];

/** Replaces secret-looking substrings (install keys, bearer tokens, auth headers, secret-named
 *  pairs, private keys, long token-like runs) with "[redacted]". Paths survive. */
export function redactSecrets(text: string): string {
  return REDACTIONS.reduce(
    (current, [pattern, replace]) =>
      current.replace(pattern, (match: string, ...groups: unknown[]) =>
        replace(match, ...groups.filter((group): group is string => typeof group === "string")),
      ),
    text,
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stringOrNull(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

/** The structured diagnostics of a daemon connect failure, or null when it carries none. */
export function daemonDiagnosticsFrom(error: DiagnosticsSource): DaemonDiagnostics | null {
  const diagnostics = error.details?.diagnostics;
  if (!isRecord(diagnostics)) return null;
  return {
    stage: stringOrNull(diagnostics.stage),
    socketPath: stringOrNull(diagnostics.socketPath),
    logPath: stringOrNull(diagnostics.logPath),
    reason: stringOrNull(diagnostics.reason),
    supervisorError: stringOrNull(diagnostics.supervisorError),
  };
}

/** The plain-text report "Copy diagnostics" puts on the clipboard, redacted. */
export function formatDaemonDiagnostics(error: DiagnosticsSource): string {
  const lines = [
    "Ferryx daemon diagnostics",
    `code: ${error.code}`,
    `message: ${error.message}`,
  ];
  if (error.details !== undefined) {
    lines.push("details:", JSON.stringify(error.details, null, 2));
  }
  return redactSecrets(lines.join("\n"));
}
