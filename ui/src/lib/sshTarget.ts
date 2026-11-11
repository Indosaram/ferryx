/**
 * Turns what people actually paste into the SSH machine form (an `ssh ...` command line, an
 * `ssh://` URI, or a bare `user@host:port`) into the form's separate connection fields.
 */
export interface ParsedSshTarget {
  hostname: string;
  username?: string;
  port?: number;
  jumpHost?: string;
  identityFile?: string;
}

// ssh(1) flags that take a value; every other single-letter flag is a boolean switch.
const FLAGS_WITH_VALUE = new Set("BbcDEeFIiJLlmOoPpQRSWw".split(""));

function tokenize(text: string): string[] | null {
  const tokens: string[] = [];
  let current = "";
  let inToken = false;
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      if (ch === quote) quote = null;
      else if (ch === "\\" && quote === '"' && i + 1 < text.length) current += text[++i];
      else current += ch;
    } else if (ch === "'" || ch === '"') {
      quote = ch;
      inToken = true;
    } else if (ch === "\\" && i + 1 < text.length) {
      const next = text[++i];
      if (next !== "\n") {
        current += next;
        inToken = true;
      }
    } else if (/\s/.test(ch)) {
      if (inToken) tokens.push(current);
      current = "";
      inToken = false;
    } else {
      current += ch;
      inToken = true;
    }
  }
  if (quote) return null;
  if (inToken) tokens.push(current);
  return tokens;
}

function parsePort(value: string): number | undefined {
  if (!/^\d{1,5}$/.test(value)) return undefined;
  const port = Number(value);
  return port >= 1 && port <= 65535 ? port : undefined;
}

/** `[user@]host[:port]`, `[user@][v6]:port`, or `ssh://[user@]host[:port]`. */
function parseDestination(raw: string): ParsedSshTarget | null {
  let value = raw.replace(/^ssh:\/\//i, "").replace(/\/+$/, "");
  let username: string | undefined;
  const at = value.lastIndexOf("@");
  if (at >= 0) {
    username = value.slice(0, at) || undefined;
    value = value.slice(at + 1);
  }
  let hostname = value;
  let port: number | undefined;
  const bracketed = /^\[([^\]]+)\](?::(\d+))?$/.exec(value);
  if (bracketed) {
    hostname = bracketed[1];
    if (bracketed[2] !== undefined) {
      port = parsePort(bracketed[2]);
      if (port === undefined) return null;
    }
  } else {
    const colon = value.split(":");
    // One colon is host:port; more than one is a bare IPv6 address.
    if (colon.length === 2) {
      hostname = colon[0];
      port = parsePort(colon[1]);
      if (port === undefined) return null;
    }
  }
  if (!hostname || /[\s/@]/.test(hostname) || hostname.startsWith("-")) return null;
  if (username !== undefined && /[\s:]/.test(username)) return null;
  return { hostname, username, port };
}

function applyOption(target: Partial<ParsedSshTarget>, keyValue: string) {
  const match = /^([A-Za-z]+)\s*[=\s]\s*(.+)$/.exec(keyValue.trim());
  if (!match) return;
  const value = match[2].trim();
  switch (match[1].toLowerCase()) {
    case "user": target.username = value; break;
    case "port": target.port = parsePort(value) ?? target.port; break;
    case "hostname": target.hostname = value; break;
    case "proxyjump": if (value.toLowerCase() !== "none") target.jumpHost = value; break;
    case "identityfile": target.identityFile = value; break;
  }
}

/**
 * Returns the connection fields described by `input`, or null when it is not an SSH target.
 * Explicit flags (`-l`, `-p`, `-o User=`, `-o Port=`) win over the destination, as in ssh(1).
 */
export function parseSshTarget(input: string): ParsedSshTarget | null {
  const tokens = tokenize(input.trim());
  if (!tokens || tokens.length === 0) return null;
  if (tokens[0] === "ssh" || /[/\\]ssh(\.exe)?$/i.test(tokens[0])) tokens.shift();

  const flags: Partial<ParsedSshTarget> = {};
  let destination: ParsedSshTarget | null = null;
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i];
    if (token === "--") {
      if (!destination && i + 1 < tokens.length) destination = parseDestination(tokens[i + 1]);
      break;
    }
    if (token.startsWith("-") && token.length > 1) {
      // Walk a cluster like `-At` or `-p2222`: booleans until the first flag that takes a value.
      for (let j = 1; j < token.length; j += 1) {
        const flag = token[j];
        if (!FLAGS_WITH_VALUE.has(flag)) continue;
        const inline = token.slice(j + 1);
        const value = inline !== "" ? inline : tokens[++i];
        if (value === undefined) return null;
        switch (flag) {
          case "J": flags.jumpHost = value; break;
          case "l": flags.username = value; break;
          case "p": { const port = parsePort(value); if (port === undefined) return null; flags.port = port; break; }
          case "i": flags.identityFile = value; break;
          case "o": applyOption(flags, value); break;
        }
        break;
      }
      continue;
    }
    // ssh(1) keeps reading options after the destination; the next operand starts the remote command.
    if (destination) break;
    destination = parseDestination(token);
    if (!destination) return null;
  }
  if (!destination) return null;

  const result: ParsedSshTarget = { hostname: flags.hostname ?? destination.hostname };
  const username = flags.username ?? destination.username;
  const port = flags.port ?? destination.port;
  if (username) result.username = username;
  if (port !== undefined) result.port = port;
  if (flags.jumpHost) result.jumpHost = flags.jumpHost;
  if (flags.identityFile) result.identityFile = flags.identityFile;
  return result;
}

/**
 * True when `text` carries more than a hostname (a user, port, flag or `ssh` prefix), so typing
 * a plain hostname into the Hostname field is never rewritten.
 */
export function looksLikeSshTarget(text: string): boolean {
  const trimmed = text.trim();
  return /^ssh(\s|:\/\/)/i.test(trimmed) || /[@\s]/.test(trimmed) || /^[^:[\]]+:\d+$/.test(trimmed) || /^\[[^\]]+\]:\d+$/.test(trimmed);
}
