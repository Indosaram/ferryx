#!/usr/bin/env bun
/**
 * Fails when a file listed in ui/i18n-enforced.txt contains a hex color literal or a JSX
 * text literal (user-visible copy that belongs in ui/src/i18n/en.json).
 *
 *   bun ui/scripts/check-hardcoded.ts      exit 0 clean, 1 with one `file:line: message` per finding
 *
 * Allowed in enforced files: punctuation-only text, values passed as interpolation (product
 * and agent names held in constants), and `data-testid` values. Comments are never checked.
 */
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const uiRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

const HEX_COLOR = /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![0-9a-zA-Z_-])/;
/** Text with at least one letter or digit is copy; anything else is punctuation or glyphs. */
const WORDY = /[\p{L}\p{N}]/u;
/** JSX attributes whose string value a user sees or a screen reader announces. */
const COPY_ATTRIBUTES = new Set(["aria-label", "aria-description", "aria-placeholder", "aria-roledescription", "aria-valuetext", "alt", "label", "placeholder", "title"]);

export type Finding = { file: string; line: number; message: string };

/** Reads ui/i18n-enforced.txt: one path per line relative to ui/, `#` starts a comment. */
export function readEnforcedList(root = uiRoot): string[] {
  return readFileSync(join(root, "i18n-enforced.txt"), "utf8")
    .split("\n")
    .map((line) => line.replace(/#.*/, "").trim())
    .filter((line) => line.length > 0);
}

function lineOf(source: ts.SourceFile, node: ts.Node): number {
  return source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
}

function preview(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > 40 ? `${flat.slice(0, 37)}...` : flat;
}

/** Checks one source text; `file` is used for the report and to pick the TSX parser. */
export function checkSource(file: string, text: string): Finding[] {
  const kind = file.endsWith(".tsx") ? ts.ScriptKind.TSX : file.endsWith(".ts") ? ts.ScriptKind.TS : ts.ScriptKind.TSX;
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, kind);
  const findings: Finding[] = [];
  const report = (node: ts.Node, message: string) => findings.push({ file, line: lineOf(source, node), message });

  const visit = (node: ts.Node) => {
    if (
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node) ||
      ts.isJsxText(node)
    ) {
      const match = HEX_COLOR.exec(node.text);
      if (match) report(node, `hex color literal ${match[0]}; use a token from ui/src/tokens`);
    }

    if (ts.isJsxText(node) && WORDY.test(node.text)) {
      report(node, `JSX text literal "${preview(node.text)}"; move it to ui/src/i18n/en.json`);
    }

    if (ts.isJsxExpression(node) && node.expression && (ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent))) {
      const inner = node.expression;
      if ((ts.isStringLiteral(inner) || ts.isNoSubstitutionTemplateLiteral(inner)) && WORDY.test(inner.text)) {
        report(inner, `JSX text literal "${preview(inner.text)}"; move it to ui/src/i18n/en.json`);
      }
    }

    if (ts.isJsxAttribute(node) && node.initializer) {
      const name = node.name.getText(source);
      const init = node.initializer;
      const literal = ts.isStringLiteral(init)
        ? init
        : ts.isJsxExpression(init) && init.expression && (ts.isStringLiteral(init.expression) || ts.isNoSubstitutionTemplateLiteral(init.expression))
          ? init.expression
          : null;
      if (literal && COPY_ATTRIBUTES.has(name) && WORDY.test(literal.text)) {
        report(literal, `JSX ${name} literal "${preview(literal.text)}"; move it to ui/src/i18n/en.json`);
      }
    }

    ts.forEachChild(node, visit);
  };

  visit(source);
  return findings;
}

export function checkEnforcedFiles(root = uiRoot): Finding[] {
  return readEnforcedList(root).flatMap((relativePath) => {
    const text = readFileSync(resolve(root, relativePath), "utf8");
    return checkSource(join("ui", relativePath), text);
  });
}

function main(): number {
  const files = readEnforcedList();
  const findings = checkEnforcedFiles();
  for (const finding of findings) console.error(`${finding.file}:${finding.line}: ${finding.message}`);
  if (findings.length > 0) {
    console.error(`check-hardcoded: ${findings.length} finding(s) in ${files.length} enforced file(s)`);
    return 1;
  }
  console.log(`check-hardcoded: ${files.length} enforced file(s) clean`);
  return 0;
}

const entry = process.argv[1];
if (entry && resolve(entry) === fileURLToPath(import.meta.url)) process.exit(main());
