import { parseThemeDocument, type ThemeDocumentV1, type ThemeValidationError } from "./themeDocument";
import {
  createVividThemeColors,
  getStandardThemeColors,
  isReservedThemeId,
  parseT3ThemeFile,
  readableThemeColorOn,
  themeColorToHex,
  themeDefinitionToDocument,
  themeIdFromName,
  THEME_FILE_VERSION,
  type ThemeAppearance,
  type ThemeColorRole,
  type ThemeDefinition,
} from "./themePalettes";

type VsCodeRgba = { r: number; g: number; b: number; a: number };
type VsCodeRgb = { r: number; g: number; b: number };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function stripJsonc(source: string): string {
  let out = "";
  let i = 0;
  let inString = false;
  let escaped = false;
  while (i < source.length) {
    const ch = source[i];
    if (inString) {
      out += ch;
      if (escaped) {
        escaped = false;
      } else if (ch === "\\") {
        escaped = true;
      } else if (ch === '"') {
        inString = false;
      }
      i += 1;
      continue;
    }
    if (ch === '"') {
      inString = true;
      out += ch;
      i += 1;
      continue;
    }
    if (ch === "/" && source[i + 1] === "/") {
      i += 2;
      while (i < source.length && source[i] !== "\n" && source[i] !== "\r") i += 1;
      continue;
    }
    if (ch === "/" && source[i + 1] === "*") {
      i += 2;
      while (i < source.length && !(source[i] === "*" && source[i + 1] === "/")) i += 1;
      i += 2;
      continue;
    }
    out += ch;
    i += 1;
  }
  let cleaned = "";
  inString = false;
  escaped = false;
  for (let j = 0; j < out.length; j += 1) {
    const ch = out[j];
    if (inString) {
      cleaned += ch;
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') {
      inString = true;
      cleaned += ch;
      continue;
    }
    if (ch === ",") {
      let k = j + 1;
      while (k < out.length && /\s/.test(out[k])) k += 1;
      if (out[k] === "}" || out[k] === "]") continue;
    }
    cleaned += ch;
  }
  return cleaned;
}

export function parseJsonc(source: string): unknown {
  return JSON.parse(stripJsonc(source));
}

function decodeGamma(value: number): number {
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

function encodeGamma(value: number): number {
  const clamped = Math.max(0, Math.min(1, value));
  return clamped <= 0.0031308 ? clamped * 12.92 : 1.055 * clamped ** (1 / 2.4) - 0.055;
}

function parseColorFunction(value: string): VsCodeRgba | null {
  const match = /^color\(\s*(display-p3|srgb)\s+([^)]+)\)$/i.exec(value);
  if (!match) return null;
  const [space, body] = [match[1].toLowerCase(), match[2]];
  const [channelPart, alphaPart] = body.split("/");
  const channels = channelPart
    .trim()
    .split(/\s+/)
    .map((part) => (part.endsWith("%") ? Number.parseFloat(part) / 100 : Number.parseFloat(part)));
  if (channels.length !== 3 || channels.some((channel) => !Number.isFinite(channel))) return null;
  const alphaRaw = alphaPart?.trim();
  const alpha =
    alphaRaw === undefined
      ? 1
      : alphaRaw.endsWith("%")
        ? Number.parseFloat(alphaRaw) / 100
        : Number.parseFloat(alphaRaw);
  if (!Number.isFinite(alpha)) return null;

  const [red, green, blue] = channels as [number, number, number];
  if (space === "srgb") {
    return { r: red * 255, g: green * 255, b: blue * 255, a: Math.max(0, Math.min(1, alpha)) };
  }
  const [lr, lg, lb] = [red, green, blue].map(decodeGamma) as [number, number, number];
  const srgb = [
    1.2249401762805 * lr - 0.2249401762805 * lg,
    -0.042056961239 * lr + 1.042056961239 * lg,
    -0.0196375547643 * lr - 0.0786360655012 * lg + 1.0982736202656 * lb,
  ].map((channel) => encodeGamma(channel) * 255) as [number, number, number];
  return { r: srgb[0], g: srgb[1], b: srgb[2], a: Math.max(0, Math.min(1, alpha)) };
}

function parseVsCodeColor(value: unknown): VsCodeRgba | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (trimmed.startsWith("color(")) return parseColorFunction(trimmed);
  const hex = trimmed.replace(/^#/, "");
  if (!/^(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(hex)) return null;
  const expand = (part: string) =>
    part.length === 1 ? Number.parseInt(part + part, 16) : Number.parseInt(part, 16);
  if (hex.length <= 4) {
    return {
      r: expand(hex[0]),
      g: expand(hex[1]),
      b: expand(hex[2]),
      a: hex.length === 4 ? expand(hex[3]) / 255 : 1,
    };
  }
  return {
    r: Number.parseInt(hex.slice(0, 2), 16),
    g: Number.parseInt(hex.slice(2, 4), 16),
    b: Number.parseInt(hex.slice(4, 6), 16),
    a: hex.length === 8 ? Number.parseInt(hex.slice(6, 8), 16) / 255 : 1,
  };
}

function toHex(color: VsCodeRgb): string {
  const ch = (v: number) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0");
  return `#${ch(color.r)}${ch(color.g)}${ch(color.b)}`;
}

function flattenOver(color: VsCodeRgba, base: VsCodeRgb): string {
  if (color.a >= 1) return toHex(color);
  return toHex({
    r: color.r * color.a + base.r * (1 - color.a),
    g: color.g * color.a + base.g * (1 - color.a),
    b: color.b * color.a + base.b * (1 - color.a),
  });
}

function relativeLuminance(color: VsCodeRgb): number {
  const ch = (v: number) => {
    const ratio = v / 255;
    return ratio <= 0.03928 ? ratio / 12.92 : ((ratio + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(color.r) + 0.7152 * ch(color.g) + 0.0722 * ch(color.b);
}

function contrastRatio(first: VsCodeRgb, second: VsCodeRgb): number {
  const a = relativeLuminance(first);
  const b = relativeLuminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

function hexToRgb(value: string): VsCodeRgb {
  return parseVsCodeColor(themeColorToHex(value) ?? value) ?? { r: 0, g: 0, b: 0, a: 1 };
}

export function isVsCodeThemeFile(value: unknown): boolean {
  if (!isRecord(value)) return false;
  if (value.version === THEME_FILE_VERSION && (isRecord(value.tokens) || typeof value.appearance === "string")) {
    return false;
  }
  const hasWorkbenchColors =
    isRecord(value.colors) && Object.keys(value.colors).some((key) => key.includes("."));
  return hasWorkbenchColors || Array.isArray(value.tokenColors);
}

export function isT3ThemeFormat(value: unknown): boolean {
  if (!isRecord(value)) return false;
  if (typeof value.appearance === "string" && (value.appearance === "light" || value.appearance === "dark")) {
    return isRecord(value.colors) || (typeof value.canvas === "string" && typeof value.accent === "string");
  }
  return false;
}

export function humanizeThemeName(raw: string): string {
  const trimmed = raw.trim();
  if (/\s/.test(trimmed) || !/[-_.]/.test(trimmed)) return trimmed;
  return trimmed
    .split(/[-_.]+/)
    .filter(Boolean)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

function resolveAppearance(value: Record<string, unknown>, canvas: VsCodeRgb): ThemeAppearance {
  const type = typeof value.type === "string" ? value.type.toLowerCase() : null;
  if (type === "light" || type === "hc-light" || type === "hclight") return "light";
  if (type === "dark" || type === "hc-black" || type === "hc") return "dark";
  return relativeLuminance(canvas) < 0.179 ? "dark" : "light";
}

function resolveName(value: Record<string, unknown>, fallbackName?: string): string {
  for (const candidate of [value.displayName, value.name, fallbackName]) {
    if (typeof candidate !== "string") continue;
    const humanized = humanizeThemeName(candidate);
    if (humanized.length > 0) return humanized.slice(0, 48);
  }
  return "VS Code theme";
}

export type AnyThemeImportResult =
  | { ok: true; doc: ThemeDocumentV1; format: "ferryx" | "t3code" | "vscode" }
  | { ok: false; errors: ThemeValidationError[] };

export function parseVsCodeThemeFile(value: unknown, fallbackName?: string): AnyThemeImportResult {
  if (!isRecord(value)) {
    return { ok: false, errors: [{ path: "$", message: "Theme files must contain a JSON object." }] };
  }
  const colors = isRecord(value.colors) ? value.colors : {};
  const pick = (...keys: ReadonlyArray<string>): VsCodeRgba | null => {
    for (const key of keys) {
      const parsed = parseVsCodeColor(colors[key]);
      if (parsed) return parsed;
    }
    return null;
  };
  const solidOver = (base: VsCodeRgb, ...keys: ReadonlyArray<string>): string | null => {
    const parsed = pick(...keys);
    return parsed ? flattenOver(parsed, base) : null;
  };

  const canvasColor = pick("editor.background", "editorPane.background");
  if (!canvasColor) {
    return {
      ok: false,
      errors: [
        {
          path: "colors.editor.background",
          message: 'That VS Code theme has no "editor.background" color to build a palette from.',
        },
      ],
    };
  }
  const canvas = { r: canvasColor.r, g: canvasColor.g, b: canvasColor.b };
  const appearance = resolveAppearance(value, canvas);
  const canvasHex = toHex(canvas);
  const raisedSurfaceCandidateHex = solidOver(canvas, "editorWidget.background", "dropdown.background");
  const standsApart = (a: string, b: string) => contrastRatio(hexToRgb(a), hexToRgb(b)) >= 1.1;

  let accentColor: VsCodeRgba | null = null;
  let accentHex: string | null = null;
  for (const key of [
    "focusBorder",
    "button.background",
    "textLink.foreground",
    "activityBarBadge.background",
    "progressBar.background",
    "badge.background",
  ]) {
    const candidate = parseVsCodeColor(colors[key]);
    if (!candidate) continue;
    const candidateHex = flattenOver(candidate, canvas);
    if (
      !standsApart(candidateHex, canvasHex) ||
      (raisedSurfaceCandidateHex !== null && !standsApart(candidateHex, raisedSurfaceCandidateHex))
    ) {
      continue;
    }
    accentColor = candidate;
    accentHex = candidateHex;
    break;
  }
  if (!accentColor || !accentHex) {
    const standardAccentHex = themeColorToHex(getStandardThemeColors(appearance).accent) ?? "#346bf1";
    accentHex =
      [standardAccentHex, "#ffffff", "#000000"].find(
        (candidate) =>
          standsApart(candidate, canvasHex) &&
          (raisedSurfaceCandidateHex === null || standsApart(candidate, raisedSurfaceCandidateHex)),
      ) ?? standardAccentHex;
    accentColor = parseVsCodeColor(accentHex)!;
  }

  const mutedAccentHex = flattenOver(
    { r: accentColor.r, g: accentColor.g, b: accentColor.b, a: 0.2 },
    canvas,
  );
  const derived = createVividThemeColors(appearance, canvasHex, mutedAccentHex);
  const sidebarHex = solidOver(canvas, "sideBar.background", "activityBar.background") ?? derived.sidebar;
  const sidebar = hexToRgb(sidebarHex);
  const terminalHex = solidOver(canvas, "terminal.background", "panel.background") ?? derived.terminalBackground;
  const terminal = hexToRgb(terminalHex);

  const readableOn = (surface: string, fallback: string, ...keys: ReadonlyArray<string>): string => {
    const surfaceRgb = hexToRgb(surface);
    const isReadable = (candidate: string) => contrastRatio(hexToRgb(candidate), surfaceRgb) >= 4.5;
    const specified = solidOver(surfaceRgb, ...keys);
    if (specified && isReadable(specified)) return specified;
    if (isReadable(fallback)) return fallback;
    return relativeLuminance(surfaceRgb) < 0.179 ? "#ffffff" : "#000000";
  };

  const readableStatusColor = (fallback: string, ...keys: ReadonlyArray<string>): string => {
    const specified = solidOver(canvas, ...keys);
    const base = specified && standsApart(specified, canvasHex) ? specified : fallback;
    return readableThemeColorOn(base, canvasHex, 4.5);
  };

  const surfaceRaisedHex = raisedSurfaceCandidateHex ?? derived.surfaceRaised;
  const buttonHex = solidOver(canvas, "button.background");
  const actionHex =
    buttonHex && standsApart(buttonHex, canvasHex) && standsApart(buttonHex, surfaceRaisedHex)
      ? buttonHex
      : accentHex;
  const inputCandidates = [
    derived.input,
    derived.surfaceRaised,
    getStandardThemeColors(appearance).input,
    "#000000",
    "#ffffff",
    "#808080",
  ];
  let inputHex =
    inputCandidates.find((c) => standsApart(c, canvasHex) && standsApart(c, actionHex)) ?? "#808080";
  for (const key of ["input.background", "input.border"]) {
    const candidate = solidOver(canvas, key);
    if (candidate && standsApart(candidate, canvasHex) && standsApart(candidate, actionHex)) {
      inputHex = candidate;
      break;
    }
  }
  const searchMatchBackground =
    solidOver(canvas, "editor.findMatchHighlightBackground") ?? derived.searchMatchBackground;
  const searchMatchActiveBackground =
    solidOver(canvas, "editor.findMatchBackground") ?? derived.searchMatchActiveBackground;

  const overrides: Partial<Record<ThemeColorRole, string>> = {
    canvas: canvasHex,
    text: readableOn(canvasHex, derived.text, "editor.foreground", "foreground"),
    textMuted: readableOn(canvasHex, derived.textMuted, "descriptionForeground", "disabledForeground"),
    surface: solidOver(canvas, "editorWidget.background") ?? derived.surface,
    surfaceRaised: surfaceRaisedHex,
    surfaceOverlay:
      solidOver(canvas, "menu.background", "quickInput.background", "dropdown.background") ??
      derived.surfaceOverlay,
    border: solidOver(canvas, "panel.border", "editorGroup.border", "contrastBorder") ?? derived.border,
    input: inputHex,
    placeholder: readableOn(surfaceRaisedHex, derived.placeholder, "input.placeholderForeground"),
    error: readableStatusColor(derived.error, "editorError.foreground", "errorForeground"),
    warning: readableStatusColor(derived.warning, "editorWarning.foreground"),
    accentSurface:
      solidOver(canvas, "list.activeSelectionBackground", "list.hoverBackground") ?? derived.accentSurface,
    codeBackground: solidOver(canvas, "textCodeBlock.background") ?? derived.codeBackground,
    searchMatchBackground,
    searchMatchForeground: readableOn(
      searchMatchBackground,
      derived.searchMatchForeground,
      "editor.findMatchHighlightForeground",
      "editor.foreground",
      "foreground",
    ),
    searchMatchActiveBackground,
    searchMatchActiveForeground: readableOn(
      searchMatchActiveBackground,
      derived.searchMatchActiveForeground,
      "editor.findMatchForeground",
      "editor.foreground",
      "foreground",
    ),
    sidebar: sidebarHex,
    sidebarForeground: readableOn(sidebarHex, derived.sidebarForeground, "sideBar.foreground"),
    sidebarBorder: solidOver(sidebar, "sideBar.border") ?? derived.sidebarBorder,
    sidebarRowHover: solidOver(sidebar, "list.hoverBackground") ?? derived.sidebarRowHover,
    sidebarRowActive:
      solidOver(sidebar, "list.inactiveSelectionBackground", "list.hoverBackground") ?? derived.sidebarRowActive,
    sidebarRowSelected: solidOver(sidebar, "list.activeSelectionBackground") ?? derived.sidebarRowSelected,
    terminalBackground: terminalHex,
    terminalForeground: readableOn(terminalHex, derived.terminalForeground, "terminal.foreground"),
    terminalCursor:
      solidOver(terminal, "terminalCursor.foreground", "editorCursor.foreground") ?? derived.terminalCursor,
    terminalSelection:
      solidOver(terminal, "terminal.selectionBackground", "editor.selectionBackground") ??
      derived.terminalSelection,
    terminalScrollbar: solidOver(terminal, "scrollbarSlider.background") ?? derived.terminalScrollbar,
    accent: accentHex,
    focus: accentHex,
    messageAction: actionHex,
    messageActionForeground: readableOn(actionHex, derived.messageActionForeground, "button.foreground"),
    accentForeground: readableOn(accentHex, derived.accentForeground, "button.foreground"),
  };

  const name = resolveName(value, fallbackName);
  const baseId = themeIdFromName(name);
  const id = isReservedThemeId(baseId) ? `${baseId}-vscode` : baseId;
  const definition: ThemeDefinition = {
    id,
    label: name,
    appearance,
    colors: { ...derived, ...overrides },
  };
  return {
    ok: true,
    format: "vscode",
    doc: themeDefinitionToDocument(definition, { canvas: canvasHex, accent: accentHex }),
  };
}

export function parseAnyThemeImport(source: string, fallbackName?: string): AnyThemeImportResult {
  let raw: unknown;
  try {
    raw = parseJsonc(source);
  } catch {
    return { ok: false, errors: [{ path: "$", message: "Expected valid JSON or JSONC." }] };
  }
  if (isVsCodeThemeFile(raw)) {
    return parseVsCodeThemeFile(raw, fallbackName);
  }
  if (isT3ThemeFormat(raw)) {
    const parsed = parseT3ThemeFile(raw);
    return parsed.ok
      ? { ok: true, doc: parsed.doc, format: "t3code" }
      : { ok: false, errors: parsed.errors };
  }
  const ferryx = parseThemeDocument(raw);
  return ferryx.ok
    ? { ok: true, doc: ferryx.doc, format: "ferryx" }
    : { ok: false, errors: ferryx.errors };
}
