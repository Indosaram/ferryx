export type ThemeDocumentV1 = {
  version: 1;
  id: string;
  name: string;
  base: "dark" | "light";
  tokens: Record<string, string>;
  css?: string;
  /** Optional opposite-appearance token overrides so one theme can support both light and dark. */
  variants?: Partial<Record<"dark" | "light", Record<string, string>>>;
  /** Optional 61-role T3 Code color map for lossless editing and export. */
  roles?: Partial<Record<string, string>>;
  variantRoles?: Partial<Record<"dark" | "light", Partial<Record<string, string>>>>;
  /** Optional seed colors used by the OKLCH palette generator. */
  seed?: { canvas: string; accent: string };
  /** Optional extension/collection provenance (e.g. OpenVSX or curated group). */
  collection?: { id: string; label: string };
  /** "app" applies CSS app-wide via T3-style remote-fetch sanitizer; default ("chat") uses chat-only scoping. */
  cssScope?: "chat" | "app";
};

export type ThemeValidationError = { path: string; message: string };
export type ThemeDocumentResult =
  | { ok: true; doc: ThemeDocumentV1 }
  | { ok: false; errors: ThemeValidationError[] };
export type ThemeColor = [red: number, green: number, blue: number, alpha: number];

/** Only colour declarations in index.css's two palette roots. RGB companions are derived, never authored. */
export const THEME_TOKEN_ALLOWLIST: ReadonlySet<string> = new Set([
  "--surface-hairline", "--surface-highlight",
  "--background", "--foreground", "--card", "--panel", "--card-foreground", "--popover", "--popover-foreground",
  "--primary", "--primary-foreground", "--secondary", "--secondary-foreground", "--muted", "--muted-foreground",
  "--accent", "--surface-hover", "--accent-foreground", "--destructive", "--destructive-foreground",
  "--border", "--input", "--ring",
  "--sidebar", "--sidebar-foreground", "--sidebar-primary", "--sidebar-primary-foreground",
  "--sidebar-accent", "--sidebar-accent-foreground", "--sidebar-border", "--sidebar-ring",
  "--worktree-sidebar", "--worktree-sidebar-foreground", "--worktree-sidebar-accent",
  "--worktree-sidebar-accent-foreground", "--worktree-sidebar-border", "--worktree-sidebar-ring",
  "--terminal", "--terminal-divider",
  ...["working", "warning", "success", "idle"].flatMap((status) =>
    ["", "-solid", "-container", "-border", "-on-container"].map((suffix) => `--status-${status}${suffix}`)),
  ...["screen", "surface", "surface-raised", "surface-hover", "user-bubble", "foreground",
    "foreground-secondary", "foreground-tertiary", "border", "primary", "primary-foreground", "link", "code",
    "danger", "row-hover", "composer-panel", "composer-surface", "composer-border", "header", "header-border"]
    .map((token) => `--chat-${token}`),
]);

const NUMBER = /^[+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?$/i;
const clamp = (value: number, max = 1) => Math.min(max, Math.max(0, value));

function numeric(value: string, percentScale: number): number | null {
  const percentage = value.endsWith("%");
  const text = percentage ? value.slice(0, -1) : value;
  if (!NUMBER.test(text)) return null;
  const result = Number(text) * (percentage ? percentScale / 100 : 1);
  return Number.isFinite(result) ? result : null;
}

function hue(value: string): number | null {
  const match = /^(.*?)(deg|grad|rad|turn)?$/.exec(value);
  if (!match || !NUMBER.test(match[1])) return null;
  const degrees = Number(match[1]) * ({ deg: 1, grad: .9, rad: 180 / Math.PI, turn: 360 }[match[2] ?? "deg"] ?? 1);
  return Number.isFinite(degrees) ? ((degrees % 360) + 360) % 360 : null;
}

/** Strict supported CSS colour syntax, independent of browser CSS parsers; RGB is rounded/clamped to sRGB. */
export function parseThemeColor(raw: string): ThemeColor | null {
  const value = raw.trim().toLowerCase();
  if (/^#(?:[\da-f]{3,4}|[\da-f]{6}|[\da-f]{8})$/.test(value)) {
    let hex = value.slice(1);
    if (hex.length <= 4) hex = [...hex].map((char) => char + char).join("");
    return [parseInt(hex.slice(0, 2), 16), parseInt(hex.slice(2, 4), 16), parseInt(hex.slice(4, 6), 16),
      hex.length === 8 ? parseInt(hex.slice(6, 8), 16) / 255 : 1];
  }
  const match = /^(rgb|rgba|hsl|hsla|oklch)\(([^()]*)\)$/.exec(value);
  if (!match) return null;
  const [, kind, body] = match;
  const comma = body.includes(",");
  if (comma && (kind === "oklch" || body.includes("/"))) return null;
  let channels: string[];
  let alphaText: string | undefined;
  if (comma) {
    const parts = body.split(",").map((part) => part.trim());
    if (parts.length !== (kind.endsWith("a") ? 4 : 3)) return null;
    channels = parts.slice(0, 3);
    alphaText = parts[3];
    // Legacy RGB does not mix percentage and number channels.
    if (kind.startsWith("rgb") && channels.some((part) => part.endsWith("%")) && !channels.every((part) => part.endsWith("%"))) return null;
  } else {
    const parts = body.split("/");
    if (parts.length > 2) return null;
    channels = parts[0].trim().split(/\s+/);
    alphaText = parts[1]?.trim();
    if (channels.length !== 3 || alphaText === "") return null;
  }
  const alpha = alphaText === undefined ? 1 : numeric(alphaText, 1);
  if (alpha === null) return null;
  let rgb: number[];
  if (kind.startsWith("rgb")) {
    const values = channels.map((channel) => numeric(channel, 255));
    if (values.some((channel) => channel === null)) return null;
    rgb = values as number[];
  } else if (kind.startsWith("hsl")) {
    const h = hue(channels[0]);
    if (h === null || !channels[1].endsWith("%") || !channels[2].endsWith("%")) return null;
    const s = numeric(channels[1], 1);
    const l = numeric(channels[2], 1);
    if (s === null || l === null) return null;
    const saturation = clamp(s);
    const lightness = clamp(l);
    const a = saturation * Math.min(lightness, 1 - lightness);
    rgb = [0, 8, 4].map((offset) => {
      const k = (offset + h / 30) % 12;
      return 255 * (lightness - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)));
    });
  } else {
    const l = numeric(channels[0], 1);
    const c = numeric(channels[1], .4);
    const h = hue(channels[2]);
    if (l === null || c === null || h === null) return null;
    const a = Math.max(0, c) * Math.cos(h * Math.PI / 180);
    const b = Math.max(0, c) * Math.sin(h * Math.PI / 180);
    const lightness = clamp(l);
    const ll = (lightness + .3963377774 * a + .2158037573 * b) ** 3;
    const mm = (lightness - .1055613458 * a - .0638541728 * b) ** 3;
    const ss = (lightness - .0894841775 * a - 1.291485548 * b) ** 3;
    rgb = [4.0767416621 * ll - 3.3077115913 * mm + .2309699292 * ss,
      -1.2684380046 * ll + 2.6097574011 * mm - .3413193965 * ss,
      -.0041960863 * ll - .7034186147 * mm + 1.707614701 * ss]
      .map((linear) => 255 * (linear <= .0031308 ? 12.92 * linear : 1.055 * linear ** (1 / 2.4) - .055));
  }
  return [Math.round(clamp(rgb[0], 255)), Math.round(clamp(rgb[1], 255)), Math.round(clamp(rgb[2], 255)), clamp(alpha)];
}

function over(foreground: ThemeColor, background: ThemeColor): ThemeColor {
  const alpha = foreground[3] + background[3] * (1 - foreground[3]);
  if (!alpha) return [0, 0, 0, 0];
  return [0, 1, 2].map((i) =>
    (foreground[i] * foreground[3] + background[i] * background[3] * (1 - foreground[3])) / alpha)
    .concat(alpha) as ThemeColor;
}

function luminance(color: ThemeColor): number {
  const values = color.slice(0, 3).map((channel) => {
    const value = channel / 255;
    return value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4;
  });
  return .2126 * values[0] + .7152 * values[1] + .0722 * values[2];
}

function contrastErrors(base: "dark" | "light", colors: Map<string, ThemeColor>): ThemeValidationError[] {
  // --chat-screen is the existing chat canvas (there is no --chat-background token).
  const defaults: Record<string, string> = base === "dark"
    ? { "--chat-screen": "#0a0a0a", "--chat-surface": "#111111", "--chat-foreground": "#f5f5f5", "--chat-primary": "#346bf1", "--chat-primary-foreground": "#ffffff" }
    : { "--chat-screen": "#ffffff", "--chat-surface": "#f4f4f5", "--chat-foreground": "#0a0a0a", "--chat-primary": "#1d4ed8", "--chat-primary-foreground": "#ffffff" };
  const color = (token: string): ThemeColor => colors.get(token) ?? parseThemeColor(defaults[token])!;
  const canvas: ThemeColor = base === "dark" ? [10, 10, 10, 1] : [255, 255, 255, 1];
  const screen = over(color("--chat-screen"), canvas);
  const errors: ThemeValidationError[] = [];
  for (const [foreground, background] of [
    ["--chat-foreground", "--chat-surface"], ["--chat-foreground", "--chat-screen"],
    ["--chat-primary-foreground", "--chat-primary"],
  ]) {
    const bg = over(color(background), screen);
    const fg = over(color(foreground), bg);
    const a = luminance(fg);
    const b = luminance(bg);
    const ratio = (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
    if (ratio < 4.5) errors.push({ path: `tokens.${foreground}`, message: `${foreground} on ${background} must reach 4.5:1 contrast (received ${ratio.toFixed(2)}:1).` });
  }
  return errors;
}

/** Validates untrusted JSON values atomically: no throwing or dropping invalid fields. CSS scoping belongs to the applier. */
export function parseThemeDocument(raw: unknown): ThemeDocumentResult {
  const errors: ThemeValidationError[] = [];
  const error = (path: string, message: string) => { errors.push({ path, message }); };
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return { ok: false, errors: [{ path: "$", message: "Expected a theme document object." }] };
  }
  const source = raw as Record<string, unknown>;
  const knownFields = new Set([
    "version", "id", "name", "base", "tokens", "css",
    "variants", "roles", "variantRoles", "seed", "collection", "cssScope",
  ]);
  for (const key of Object.keys(source)) {
    if (!knownFields.has(key)) error(key, "Unknown theme document field.");
  }
  if (source.version !== 1) error("version", "Expected theme document version 1.");
  if (typeof source.id !== "string" || !/^[a-z0-9][a-z0-9-]{1,63}$/.test(source.id)) error("id", "Use 2-64 lowercase letters, digits or hyphens, starting with a letter or digit.");
  if (typeof source.name !== "string" || !source.name.trim() || source.name.length > 64) error("name", "Expected a nonblank name of at most 64 characters.");
  if (source.base !== "dark" && source.base !== "light") error("base", "Expected dark or light.");
  const tokens: Record<string, string> = {};
  const colors = new Map<string, ThemeColor>();
  if (!source.tokens || typeof source.tokens !== "object" || Array.isArray(source.tokens)) error("tokens", "Expected a colour token object.");
  else {
    const entries = Object.entries(source.tokens);
    if (entries.length > 200) error("tokens", "At most 200 token entries are allowed.");
    for (const [key, value] of entries) {
      if (!THEME_TOKEN_ALLOWLIST.has(key)) {
        error(`tokens.${key}`, key.endsWith("-rgb")
          ? "RGB companions are derived from their colour token and are not authorable."
          : "Only allowlisted colour tokens are authorable; radius, timing and numeric tokens are not allowed.");
      } else {
        const parsed = typeof value === "string" ? parseThemeColor(value) : null;
        if (!parsed) error(`tokens.${key}`, "Expected a hex, rgb()/rgba(), hsl()/hsla() or oklch() colour; references and other values are not allowed.");
        else { tokens[key] = value as string; colors.set(key, parsed); }
      }
    }
  }
  const cssScope = source.cssScope === "app" || source.cssScope === "chat" ? source.cssScope : undefined;
  if (source.cssScope !== undefined && !cssScope) error("cssScope", 'Expected "chat" or "app".');
  const maxCssBytes = cssScope === "app" ? 100_000 : 32 * 1024;
  if (source.css !== undefined && (typeof source.css !== "string" || new TextEncoder().encode(source.css).length > maxCssBytes)) {
    error("css", cssScope === "app" ? "Expected CSS text of at most 100,000 bytes (UTF-8)." : "Expected CSS text of at most 32 KiB (UTF-8).");
  }
  let variants: Partial<Record<"dark" | "light", Record<string, string>>> | undefined;
  if (source.variants !== undefined) {
    if (!source.variants || typeof source.variants !== "object" || Array.isArray(source.variants)) {
      error("variants", "Expected a variants object.");
    } else {
      variants = {};
      for (const [mode, vTokens] of Object.entries(source.variants as Record<string, unknown>)) {
        if (mode !== "dark" && mode !== "light") {
          error(`variants.${mode}`, 'Expected "dark" or "light".');
          continue;
        }
        if (!vTokens || typeof vTokens !== "object" || Array.isArray(vTokens)) {
          error(`variants.${mode}`, "Expected a colour token object.");
          continue;
        }
        const validated: Record<string, string> = {};
        for (const [k, v] of Object.entries(vTokens as Record<string, unknown>)) {
          if (THEME_TOKEN_ALLOWLIST.has(k) && typeof v === "string" && parseThemeColor(v)) {
            validated[k] = v;
          }
        }
        variants[mode] = validated;
      }
    }
  }
  let roles: Partial<Record<string, string>> | undefined;
  if (source.roles !== undefined && source.roles && typeof source.roles === "object" && !Array.isArray(source.roles)) {
    roles = {};
    for (const [k, v] of Object.entries(source.roles as Record<string, unknown>)) {
      if (typeof v === "string" && parseThemeColor(v)) roles[k] = v;
    }
  }
  let variantRoles: Partial<Record<"dark" | "light", Partial<Record<string, string>>>> | undefined;
  if (source.variantRoles !== undefined && source.variantRoles && typeof source.variantRoles === "object" && !Array.isArray(source.variantRoles)) {
    variantRoles = {};
    for (const [mode, vRoles] of Object.entries(source.variantRoles as Record<string, unknown>)) {
      if ((mode === "dark" || mode === "light") && vRoles && typeof vRoles === "object" && !Array.isArray(vRoles)) {
        const cleaned: Partial<Record<string, string>> = {};
        for (const [k, v] of Object.entries(vRoles as Record<string, unknown>)) {
          if (typeof v === "string" && parseThemeColor(v)) cleaned[k] = v;
        }
        variantRoles[mode] = cleaned;
      }
    }
  }
  let seed: { canvas: string; accent: string } | undefined;
  if (source.seed !== undefined && source.seed && typeof source.seed === "object" && !Array.isArray(source.seed)) {
    const s = source.seed as Record<string, unknown>;
    if (typeof s.canvas === "string" && typeof s.accent === "string" && parseThemeColor(s.canvas) && parseThemeColor(s.accent)) {
      seed = { canvas: s.canvas, accent: s.accent };
    }
  }
  let collection: { id: string; label: string } | undefined;
  if (source.collection !== undefined && source.collection && typeof source.collection === "object" && !Array.isArray(source.collection)) {
    const c = source.collection as Record<string, unknown>;
    if (typeof c.id === "string" && typeof c.label === "string") {
      collection = { id: c.id, label: c.label };
    }
  }
  if (errors.length === 0 && (source.base === "dark" || source.base === "light")) errors.push(...contrastErrors(source.base, colors));
  if (errors.length) return { ok: false, errors };
  return { ok: true, doc: {
    version: 1, id: source.id as string, name: source.name as string, base: source.base as "dark" | "light", tokens,
    ...(source.css === undefined ? {} : { css: source.css as string }),
    ...(variants ? { variants } : {}),
    ...(roles ? { roles } : {}),
    ...(variantRoles ? { variantRoles } : {}),
    ...(seed ? { seed } : {}),
    ...(collection ? { collection } : {}),
    ...(cssScope ? { cssScope } : {}),
  } };
}
