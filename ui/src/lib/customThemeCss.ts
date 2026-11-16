export type CustomCssError = { path: string; message: string };
export type CustomCssResult =
  | { ok: true; css: string }
  | { ok: false; errors: CustomCssError[] };

type Rule = { prelude: string; body: string; index: number };

const PROPERTIES = new Set([
  "color", "background", "background-color", "background-image", "background-size",
  "background-position", "background-repeat", "background-clip", "background-origin",
  "border", "border-color", "border-style", "border-width", "border-radius",
  "border-top", "border-right", "border-bottom", "border-left",
  "border-top-color", "border-right-color", "border-bottom-color", "border-left-color",
  "border-top-width", "border-right-width", "border-bottom-width", "border-left-width",
  "border-top-left-radius", "border-top-right-radius", "border-bottom-left-radius", "border-bottom-right-radius",
  "box-shadow", "text-shadow", "outline", "outline-color", "outline-style", "outline-width", "outline-offset",
  "font", "font-family", "font-size", "font-weight", "font-style", "font-variant", "line-height",
  "letter-spacing", "word-spacing", "text-align", "text-decoration", "text-decoration-color",
  "text-decoration-line", "text-decoration-style", "text-transform", "text-indent", "white-space",
  "overflow-wrap", "word-break", "opacity", "display", "gap", "row-gap", "column-gap",
  "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
  "padding-inline", "padding-inline-start", "padding-inline-end", "padding-block", "padding-block-start", "padding-block-end",
  "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
  "margin-inline", "margin-inline-start", "margin-inline-end", "margin-block", "margin-block-start", "margin-block-end",
]);
const FUNCTIONS = new Set([
  "rgb", "rgba", "hsl", "hsla", "oklch", "oklab", "lab", "lch", "color", "color-mix",
  "var", "calc", "min", "max", "clamp", "linear-gradient", "radial-gradient", "conic-gradient",
  "repeating-linear-gradient", "repeating-radial-gradient", "repeating-conic-gradient", "url",
]);
const PSEUDOS = new Set([
  "hover", "active", "focus", "focus-visible", "focus-within", "disabled", "enabled", "checked",
  "first-child", "last-child", "only-child", "empty", "first-of-type", "last-of-type", "only-of-type",
  "before", "after", "placeholder", "selection", "marker",
]);

/** Explicit lexical scan: strings/comments/functions cannot hide rule or declaration delimiters. */
function splitOutside(text: string, delimiters: string): { parts: string[]; separators: string[]; error?: string } {
  const parts: string[] = [];
  const separators: string[] = [];
  let start = 0;
  let quote = "";
  const stack: string[] = [];
  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (quote) { if (char === quote) quote = ""; continue; }
    if (char === '"' || char === "'") { quote = char; continue; }
    if (char === "(" || char === "[") stack.push(char);
    else if (char === ")" || char === "]") {
      if (stack.pop() !== (char === ")" ? "(" : "[")) return { parts, separators, error: "Unbalanced delimiters." };
    } else if (!stack.length && delimiters.includes(char)) {
      parts.push(text.slice(start, i)); separators.push(char); start = i + 1;
    }
  }
  parts.push(text.slice(start));
  return { parts, separators, ...(quote || stack.length ? { error: "Unterminated string or function." } : {}) };
}

function tokenizeRules(text: string, errors: CustomCssError[], counter: { value: number }): Rule[] {
  const rules: Rule[] = [];
  let start = 0;
  let quote = "";
  let parentheses = 0;
  let depth = 0;
  let open = -1;
  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (quote) { if (char === quote) quote = ""; continue; }
    if (char === '"' || char === "'") { quote = char; continue; }
    if (char === "(") parentheses++;
    if (char === ")") parentheses--;
    if (parentheses < 0) break;
    if (parentheses) continue;
    if (char === ";" && depth === 0) {
      const index = counter.value++;
      errors.push({ path: `rule ${index}`, message: "At-rules and statements without a style block are not allowed." });
      start = i + 1;
    } else if (char === "{") {
      if (depth++ === 0) open = i;
    } else if (char === "}") {
      if (--depth < 0) break;
      if (depth === 0) {
        rules.push({ prelude: text.slice(start, open).trim(), body: text.slice(open + 1, i), index: counter.value++ });
        start = i + 1;
      }
    }
  }
  if (quote || parentheses || depth || text.slice(start).trim()) {
    errors.push({ path: `rule ${counter.value++}`, message: "Malformed or unterminated rule." });
  }
  return rules;
}

/** Deliberately plain selector grammar; no nesting, escaped identifiers or functional pseudos. */
function plainSelector(selector: string): boolean {
  let rest = selector.trim();
  let needsCompound = true;
  while (rest) {
    const space = /^\s+/.exec(rest);
    if (space) { rest = rest.slice(space[0].length); if (!rest) break; }
    if (/^[>+~]/.test(rest)) {
      if (needsCompound) return false;
      rest = rest.slice(1).trimStart(); needsCompound = true; continue;
    }
    const atom = /^(\*|[.#]?[a-zA-Z_][\w-]*|\[(?:[\w-]+)(?:\s*[~|^$*]?=\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[\w-]+))?\s*\]|::?[a-zA-Z-]+)/.exec(rest);
    if (!atom) return false;
    const value = atom[0];
    if (/^(html|body)$/i.test(value)) return false;
    if (value.startsWith(":") && !PSEUDOS.has(value.replace(/^::?/, "").toLowerCase())) return false;
    rest = rest.slice(value.length); needsCompound = false;
  }
  return !needsCompound;
}

function declarations(body: string): { css?: string; error?: string } {
  const split = splitOutside(body, ";");
  if (split.error) return { error: split.error };
  const output: string[] = [];
  for (const entry of split.parts) {
    if (!entry.trim()) continue;
    const match = /^\s*([a-z-]+)\s*:\s*([\s\S]+?)\s*$/i.exec(entry);
    if (!match) return { error: "Expected a property and value declaration." };
    const property = match[1].toLowerCase();
    const value = match[2];
    if (!PROPERTIES.has(property)) return { error: `Property ${property} is not allowed.` };
    if (/[!{}@<>]/.test(value)) return { error: "Escalation, nested rules and markup are not allowed." };
    // Inspect outside strings. Only url(data:...) may contain a resource; other function names
    // are explicit so image-set(), paint(), expression(), etc. cannot smuggle resource loads.
    let quote = "";
    for (let i = 0; i < value.length; i++) {
      const char = value[i];
      if (quote) { if (char === quote) quote = ""; continue; }
      if (char === '"' || char === "'") { quote = char; continue; }
      const fn = /^([a-z-]+)\s*\(/i.exec(value.slice(i));
      if (!fn) continue;
      const name = fn[1].toLowerCase();
      if (!FUNCTIONS.has(name)) return { error: `Function ${name} is not allowed.` };
      i += fn[0].length - 1;
      if (name === "url") {
        const url = /^\s*(?:"(data:[^"]*)"|'(data:[^']*)'|(data:[^\s()]*))\s*\)/i.exec(value.slice(i + 1));
        if (!url) return { error: "Only data: URLs are allowed." };
        i += url[0].length;
      }
    }
    output.push(`${property}: ${value}`);
  }
  if (!output.length) return { error: "Expected at least one declaration." };
  return { css: output.join("; ") };
}

/** Atomic, non-browser CSS validation. A refused sheet always returns errors and never partial CSS. */
export function scopeCustomCss(css: string, scopeId: string): CustomCssResult {
  const errors: CustomCssError[] = [];
  const fail = (message: string): CustomCssResult => ({ ok: false, errors: [{ path: "rule 0", message }] });
  if (!/^[a-z0-9][a-z0-9-]{0,63}$/.test(scopeId)) return fail("Invalid custom-theme scope id.");
  if (new TextEncoder().encode(css).length > 32 * 1024) return fail("CSS exceeds 32 KiB.");
  // Reject escapes rather than attempting to duplicate CSS's context-sensitive escape decoding.
  if (/[\\\u0000-\u0008\u000b\u000e-\u001f\u007f]/.test(css)) return fail("CSS escapes and control characters are not allowed.");
  if (/\/\*(?:(?!\*\/)[\s\S])*$/.test(css)) return fail("Unterminated comment.");
  const source = css.replace(/\/\*[\s\S]*?\*\//g, " ");
  const counter = { value: 0 };
  const prefix = `html[data-custom-theme="${scopeId}"] [data-chat-scope] `;
  const render = (rules: Rule[], inMedia = false): string => rules.map((rule) => {
    const reject = (message: string) => { errors.push({ path: `rule ${rule.index}`, message }); return ""; };
    if (rule.prelude.startsWith("@")) {
      if (inMedia || !/^@media\s+\(prefers-reduced-motion(?:\s*:\s*(?:reduce|no-preference))?\)$/i.test(rule.prelude)) {
        return reject("Only a prefers-reduced-motion @media wrapper is allowed; other at-rules are refused.");
      }
      return `${rule.prelude} { ${render(tokenizeRules(rule.body, errors, counter), true)} }`;
    }
    const selectors = splitOutside(rule.prelude, ",");
    if (selectors.error || selectors.parts.some((selector) => !plainSelector(selector))) return reject("Expected a plain selector list without :root, html or body.");
    const result = declarations(rule.body);
    if (result.error) return reject(result.error);
    return `${selectors.parts.map((selector) => prefix + selector.trim()).join(", ")} { ${result.css} }`;
  }).join("\n");
  const output = render(tokenizeRules(source, errors, counter));
  return errors.length ? { ok: false, errors } : { ok: true, css: output };
}
