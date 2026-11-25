import { THEME_COLOR_ROLES } from "./themePalettes";

const FETCHES_BY_STRING = /image-set\(|\bsrc\(/i;
const URL_NOT_LOCAL = /url\((?![ \t\n\f\r]*["']?(?:data:|#))/i;
const ESCAPE = String.raw`\\(?:(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{1,5}(?![0-9a-fA-F]))(?:\r\n|[\n\r\f])?|\r\n|[^0-9a-fA-F])`;
const STRING = String.raw`"(?:${ESCAPE}|[^"\\\n\r\f])*"|'(?:${ESCAPE}|[^'\\\n\r\f])*'`;
const URL_OR_STRING = String.raw`url\((?![ \t\n\f\r]*["'])([^)]*)|url\([ \t\n\f\r]*(${STRING})|${STRING}|${ESCAPE}`;
const CSS_TOKENS = new RegExp(String.raw`\/\*[\s\S]*?\*\/|${URL_OR_STRING}`, "gi");

function withoutStrings(code: string, stylesheet: boolean): string {
  return code.replace(CSS_TOKENS, (token, address?: string, urlString?: string) => {
    const url = stylesheet ? (address ?? urlString) : urlString;
    if (url !== undefined) {
      if (!stylesheet && url.includes("\\")) return token;
      return /^[ \t\n\f\r]*["']?(?:data:|#)/i.test(url) ? "" : "url(";
    }
    if (token.startsWith("/*")) {
      if (stylesheet) return "";
      return token.includes("\\") ? token : " ";
    }
    return /^["']/.test(token) ? "" : token;
  });
}

function fetchesOutsideStrings(code: string): boolean {
  return FETCHES_BY_STRING.test(code) || URL_NOT_LOCAL.test(code);
}

export function themeCssValueFetches(value: string): boolean {
  const code = withoutStrings(value, false);
  return code.includes("\\") || fetchesOutsideStrings(code);
}

export function themeCssLoadsRemoteFiles(css: string): boolean {
  const code = withoutStrings(css, true);
  return /@import/i.test(code) || fetchesOutsideStrings(code);
}

export const THEME_CSS_HOOKS = [
  { selector: '[data-slot="sidebar"]', styles: "app sidebar" },
  { selector: '[data-slot="sidebar-inner"]', styles: "sidebar surface" },
  { selector: "[data-thread-item]", styles: "sidebar worktree / thread row" },
  { selector: "[data-chat-header]", styles: "chat header" },
  { selector: '[data-workspace-area="conversation"]', styles: "chat area, behind the messages" },
  { selector: ".messages-timeline-scroll", styles: "chat scroller" },
  { selector: '[data-message-role="user"]', styles: "user message row" },
  { selector: "[data-user-message-bubble]", styles: "user message bubble" },
  { selector: '[data-message-role="assistant"]', styles: "assistant message row" },
  { selector: ".chat-markdown", styles: "assistant markdown prose" },
  { selector: ".chat-markdown-codeblock", styles: "code block" },
  { selector: "[data-chat-composer-form]", styles: "composer form" },
  { selector: '[data-slot="composer-host"]', styles: "composer card" },
  { selector: "[data-settings-page-scroll]", styles: "settings dialog content" },
  { selector: '[data-slot="button"]', styles: "button" },
  { selector: '[data-slot="input"]', styles: "text input" },
  { selector: '[data-slot="switch"]', styles: "switch" },
  { selector: '[data-slot="select-trigger"]', styles: "select trigger" },
  { selector: '[data-slot="dialog-popup"]', styles: "dialog surface" },
  { selector: '[data-slot="popover-popup"]', styles: "popover surface" },
] as const;

export const THEME_GUIDE = [
  "Themes restyle Ferryx on desktop and web/mobile.",
  "",
  "Colors: `canvas` and `accent` (hex) derive a full OKLCH palette. `colors` overrides single roles, and `variants.light` or `variants.dark` overrides the opposite appearance. Prefer color roles over CSS.",
  `Roles: ${THEME_COLOR_ROLES.join(", ")}.`,
  "",
  "CSS: `css` applies after the app's styles. Use `.dark` or `:root:not(.dark)` to target one appearance.",
  "It cannot load remote files: no @import, no image-set(), and url() only with data: or #id.",
  "For a background image, place a JPEG, PNG, or WebP file (up to 16 MB) at `<themesDir>/<themeId>.jpg` (or .png, .webp) beside `<themeId>.json`, then reference `var(--theme-background-image)` in CSS.",
  "If an app utility rule is more specific, add `!important`.",
  "",
  "Stable selectors:",
  ...THEME_CSS_HOOKS.map((hook) => `- ${hook.selector}: ${hook.styles}`),
  "",
  "To undo, run `ferryx-cli theme set charcoal` or `ferryx-cli theme clear`, or press Cmd+Option+A / Ctrl+Alt+A.",
].join("\n");

type RuleParent = Pick<CSSStyleSheet, "cssRules" | "deleteRule">;

function isRuleParent(rule: CSSRule): rule is CSSRule & RuleParent {
  return (
    "cssRules" in rule &&
    "deleteRule" in rule &&
    !(typeof CSSKeyframesRule !== "undefined" && rule instanceof CSSKeyframesRule)
  );
}

function stripDeclarations(rule: CSSRule): boolean {
  if (!("style" in rule) || !(rule.style instanceof CSSStyleDeclaration)) return false;
  for (let i = rule.style.length - 1; i >= 0; i -= 1) {
    const prop = rule.style.item(i);
    if (themeCssValueFetches(rule.style.getPropertyValue(prop))) {
      rule.style.removeProperty(prop);
    }
  }
  return true;
}

function stripFetches(parent: RuleParent): void {
  for (let i = parent.cssRules.length - 1; i >= 0; i -= 1) {
    const rule = parent.cssRules[i];
    if (!rule) continue;
    const hasStyle = stripDeclarations(rule);
    if (typeof CSSKeyframesRule !== "undefined" && rule instanceof CSSKeyframesRule) {
      for (const frame of Array.from(rule.cssRules)) stripDeclarations(frame);
    } else if (isRuleParent(rule)) {
      stripFetches(rule);
    } else if (typeof CSSPropertyRule !== "undefined" && rule instanceof CSSPropertyRule) {
      if (themeCssValueFetches(rule.initialValue ?? "")) parent.deleteRule(i);
    } else if (
      !hasStyle &&
      !(typeof CSSLayerStatementRule !== "undefined" && rule instanceof CSSLayerStatementRule) &&
      !(typeof CSSNamespaceRule !== "undefined" && rule instanceof CSSNamespaceRule)
    ) {
      parent.deleteRule(i);
    }
  }
}

export function stripRemoteCssTextFallback(css: string): string {
  const withoutImport = css.replace(/@import\b[^;{}]*(?:;|\{[\s\S]*?\})/gi, "");
  return withoutImport.replace(/[^;{}]*\b(?:url|image-set|src)\s*\([^;{}]*[;]?/gi, (decl) => {
    const colon = decl.indexOf(":");
    if (colon < 0) return "";
    const value = decl.slice(colon + 1);
    return themeCssValueFetches(value) ? "" : decl;
  });
}

const appliedSheets = new WeakMap<Document, { sheet?: CSSStyleSheet; styleEl?: HTMLStyleElement; css: string }>();

export function applyAppThemeCss(css: string | undefined, doc: Document = document): void {
  const next = css ?? "";
  const existing = appliedSheets.get(doc);
  if (existing && existing.css === next) return;

  if (!next) {
    clearAppThemeCss(doc);
    return;
  }

  const supportsAdopted =
    typeof CSSStyleSheet !== "undefined" &&
    "adoptedStyleSheets" in doc &&
    typeof CSSStyleSheet.prototype.replaceSync === "function";

  if (supportsAdopted) {
    try {
      let sheet = existing?.sheet;
      if (!sheet) {
        sheet = new CSSStyleSheet();
        doc.adoptedStyleSheets = [...doc.adoptedStyleSheets, sheet];
      }
      sheet.replaceSync(next);
      stripFetches(sheet);
      appliedSheets.set(doc, { sheet, css: next });
      return;
    } catch {
      // fall through to style element
    }
  }

  let styleEl = existing?.styleEl;
  if (!styleEl) {
    styleEl = doc.createElement("style");
    styleEl.dataset.ferryxAppThemeCss = "true";
    (doc.head ?? doc.documentElement).appendChild(styleEl);
  }
  styleEl.textContent = stripRemoteCssTextFallback(next);
  if (styleEl.sheet) {
    try {
      stripFetches(styleEl.sheet);
    } catch {
      return;
    }
  }
  appliedSheets.set(doc, { styleEl, css: next });
}

export function clearAppThemeCss(doc: Document = document): void {
  const existing = appliedSheets.get(doc);
  if (!existing) return;
  if (existing.sheet && "adoptedStyleSheets" in doc) {
    doc.adoptedStyleSheets = doc.adoptedStyleSheets.filter((s) => s !== existing.sheet);
  }
  existing.styleEl?.remove();
  appliedSheets.delete(doc);
}
