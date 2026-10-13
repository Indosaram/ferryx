import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function parseTokens(cssText: string, selectorRegex: RegExp): Map<string, string> {
  const match = cssText.match(selectorRegex);
  if (!match) return new Map();
  const block = match[1];
  const tokenRegex = /(--[\w-]+)\s*:\s*([^;]+);/g;
  const tokens = new Map<string, string>();
  let m: RegExpExecArray | null;
  while ((m = tokenRegex.exec(block)) !== null) {
    tokens.set(m[1].trim(), m[2].trim());
  }
  return tokens;
}

function hexToRgb(hex: string): string {
  const clean = hex.replace("#", "").trim();
  const full = clean.length === 3 ? clean.split("").map((c) => c + c).join("") : clean;
  const r = parseInt(full.slice(0, 2), 16);
  const g = parseInt(full.slice(2, 4), 16);
  const b = parseInt(full.slice(4, 6), 16);
  return `${r} ${g} ${b}`;
}

describe("Mobile chat theme tokens contract", () => {
  const cssPath = resolve(__dirname, "../../index.css");
  const css = readFileSync(cssPath, "utf-8");

  const darkTokens = parseTokens(css, /:root\s*\{([\s\S]*?)\n\}/);
  const lightTokens = parseTokens(css, /:root\[data-theme="light"\]\s*\{([\s\S]*?)\n\}/);

  it("defines every --chat-* token in the light block that exists in the root dark block", () => {
    expect(darkTokens.size).toBeGreaterThan(0);
    expect(lightTokens.size).toBeGreaterThan(0);

    const missingInLight: string[] = [];
    for (const [token] of darkTokens) {
      if (token.startsWith("--chat-") && !lightTokens.has(token)) {
        missingInLight.push(token);
      }
    }
    expect(missingInLight).toEqual([]);
  });

  it("ensures each -rgb twin in the light block numerically matches its hex counterpart", () => {
    expect(lightTokens.size).toBeGreaterThan(0);

    for (const [token, value] of lightTokens) {
      if (token.endsWith("-rgb")) {
        const hexToken = token.slice(0, -4);
        const hexValue = lightTokens.get(hexToken);
        expect(hexValue).toBeDefined();
        if (hexValue && hexValue.startsWith("#")) {
          const expectedRgb = hexToRgb(hexValue);
          expect(value).toBe(expectedRgb);
        }
      }
    }
  });

  it("ensures each -rgb twin in the dark block numerically matches its hex counterpart", () => {
    expect(darkTokens.size).toBeGreaterThan(0);

    for (const [token, value] of darkTokens) {
      if (token.endsWith("-rgb")) {
        const hexToken = token.slice(0, -4);
        const hexValue = darkTokens.get(hexToken);
        expect(hexValue).toBeDefined();
        if (hexValue && hexValue.startsWith("#")) {
          const expectedRgb = hexToRgb(hexValue);
          expect(value).toBe(expectedRgb);
        }
      }
    }
  });

  it("does not set --status-* tokens in the global :root[data-theme='light'] block", () => {
    const globalLightTokens = parseTokens(css, /:root\[data-theme="light"\]\s*\{([\s\S]*?)\n\}/);
    for (const [token] of globalLightTokens) {
      expect(token.startsWith("--status-")).toBe(false);
    }
  });

  it("defines light mode overrides for .remote-app-root with all pinned tokens, status tokens, and matching -rgb twins", () => {
    const remoteDarkTokens = parseTokens(css, /\.remote-app-root\s*\{([\s\S]*?)\n\}/);
    const remoteLightTokens = parseTokens(css, /:root\[data-theme="light"\]\s+\.remote-app-root\s*\{([\s\S]*?)\n\}/);

    expect(remoteDarkTokens.size).toBeGreaterThan(0);
    expect(remoteLightTokens.size).toBeGreaterThan(0);

    expect(remoteLightTokens.get("--status-working")).toBeDefined();
    expect(remoteLightTokens.get("--status-working-rgb")).toBeDefined();
    expect(remoteLightTokens.get("--status-warning")).toBeDefined();
    expect(remoteLightTokens.get("--status-warning-rgb")).toBeDefined();

    const missingInLight: string[] = [];
    for (const [token] of remoteDarkTokens) {
      if (!remoteLightTokens.has(token)) {
        missingInLight.push(token);
      }
    }
    expect(missingInLight).toEqual([]);

    for (const [token, value] of remoteLightTokens) {
      if (token.endsWith("-rgb")) {
        const hexToken = token.slice(0, -4);
        const hexValue = remoteLightTokens.get(hexToken);
        expect(hexValue).toBeDefined();
        if (hexValue && hexValue.startsWith("#")) {
          expect(value).toBe(hexToRgb(hexValue));
        }
      }
    }
  });
});

