import { describe, expect, it } from "vitest";

import { checkEnforcedFiles, checkSource, readEnforcedList } from "../../scripts/check-hardcoded";

describe("check-hardcoded", () => {
  it("passes every file listed in ui/i18n-enforced.txt", () => {
    expect(readEnforcedList()).toContain("src/components/onboarding/WelcomeWizard.tsx");
    expect(checkEnforcedFiles()).toEqual([]);
  });

  it("reports hex literals and JSX copy with file and line", () => {
    const source = [
      'const accent = "#ff0000";',
      "export const A = () => (",
      "  <div title=\"Close setup\" data-testid=\"plain-id\">",
      "    Hello there",
      "    {\"Inline copy\"}",
      "    {\" > \"}",
      "    <span style={{ color: `#abc` }}>:</span>",
      "  </div>",
      ");",
    ].join("\n");
    const findings = checkSource("ui/src/x.tsx", source).map((finding) => `${finding.file}:${finding.line}: ${finding.message}`);
    expect(findings).toEqual([
      "ui/src/x.tsx:1: hex color literal #ff0000; use a token from ui/src/tokens",
      'ui/src/x.tsx:3: JSX title literal "Close setup"; move it to ui/src/i18n/en.json',
      'ui/src/x.tsx:4: JSX text literal "Hello there"; move it to ui/src/i18n/en.json',
      'ui/src/x.tsx:5: JSX text literal "Inline copy"; move it to ui/src/i18n/en.json',
      "ui/src/x.tsx:7: hex color literal #abc; use a token from ui/src/tokens",
    ]);
  });

  it("ignores comments, punctuation, test ids and non-color hashes", () => {
    const source = [
      "// #ff0000 in a comment",
      'const anchor = "#section-1";',
      "export const B = () => <p data-testid=\"Some id\">{name} / {count}</p>;",
    ].join("\n");
    expect(checkSource("ui/src/y.tsx", source)).toEqual([]);
  });
});
