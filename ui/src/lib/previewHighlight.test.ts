import { describe, expect, it } from "vitest";

import { highlightLines, languageForPath } from "./previewHighlight";

describe("previewHighlight languageForPath", () => {
  it("maps common programming languages and extensions correctly", () => {
    expect(languageForPath("app.ts")).toBe("typescript");
    expect(languageForPath("component.tsx")).toBe("typescript");
    expect(languageForPath("module.mts")).toBe("typescript");
    expect(languageForPath("common.cts")).toBe("typescript");

    expect(languageForPath("index.js")).toBe("javascript");
    expect(languageForPath("view.jsx")).toBe("javascript");
    expect(languageForPath("entry.mjs")).toBe("javascript");
    expect(languageForPath("config.cjs")).toBe("javascript");

    expect(languageForPath("package.json")).toBe("json");
    expect(languageForPath("tsconfig.jsonc")).toBe("json");

    expect(languageForPath("main.rs")).toBe("rust");
    expect(languageForPath("script.py")).toBe("python");
    expect(languageForPath("server.go")).toBe("go");

    expect(languageForPath("deploy.sh")).toBe("bash");
    expect(languageForPath("run.bash")).toBe("bash");
    expect(languageForPath(".zshrc")).toBe("bash");
    expect(languageForPath(".bashrc")).toBe("bash");
    expect(languageForPath(".bash_profile")).toBe("bash");
    expect(languageForPath(".zprofile")).toBe("bash");
    expect(languageForPath(".profile")).toBe("bash");

    expect(languageForPath("config.yml")).toBe("yaml");
    expect(languageForPath("ci.yaml")).toBe("yaml");

    expect(languageForPath("style.css")).toBe("css");
    expect(languageForPath("theme.scss")).toBe("css");

    expect(languageForPath("index.html")).toBe("xml");
    expect(languageForPath("legacy.htm")).toBe("xml");
    expect(languageForPath("data.xml")).toBe("xml");
    expect(languageForPath("icon.svg")).toBe("xml");
    expect(languageForPath("Info.plist")).toBe("xml");

    expect(languageForPath("query.sql")).toBe("sql");
    expect(languageForPath("App.java")).toBe("java");

    expect(languageForPath("native.c")).toBe("c");
    expect(languageForPath("header.h")).toBe("c");

    expect(languageForPath("core.cc")).toBe("cpp");
    expect(languageForPath("engine.cpp")).toBe("cpp");
    expect(languageForPath("impl.cxx")).toBe("cpp");
    expect(languageForPath("types.hpp")).toBe("cpp");
    expect(languageForPath("defs.hh")).toBe("cpp");

    expect(languageForPath("Program.cs")).toBe("csharp");
    expect(languageForPath("script.rb")).toBe("ruby");
    expect(languageForPath("index.php")).toBe("php");
    expect(languageForPath("Main.swift")).toBe("swift");

    expect(languageForPath("Main.kt")).toBe("kotlin");
    expect(languageForPath("build.gradle.kts")).toBe("kotlin");

    expect(languageForPath("Cargo.toml")).toBe("ini");
    expect(languageForPath("settings.ini")).toBe("ini");
    expect(languageForPath("app.cfg")).toBe("ini");

    expect(languageForPath("changes.diff")).toBe("diff");
    expect(languageForPath("patch.patch")).toBe("diff");

    expect(languageForPath("Dockerfile")).toBe("dockerfile");
    expect(languageForPath("dockerfile")).toBe("dockerfile");
    expect(languageForPath("Dockerfile.dev")).toBe("dockerfile");
    expect(languageForPath("path/to/Dockerfile")).toBe("dockerfile");
  });

  it("returns null for unsupported, unknown, or markdown files", () => {
    expect(languageForPath("README.md")).toBeNull();
    expect(languageForPath("notes.txt")).toBeNull();
    expect(languageForPath("image.png")).toBeNull();
    expect(languageForPath("no-extension")).toBeNull();
  });
});

describe("previewHighlight highlightLines", () => {
  it("highlights a 3-line TS snippet with a multi-line comment with balanced spans", () => {
    const code = "/* start\nmiddle\nend */";
    const lines = highlightLines(code, "typescript");

    expect(lines).not.toBeNull();
    expect(lines).toHaveLength(3);

    for (const line of lines!) {
      const openMatches = line.match(/<span\b[^>]*>/g) || [];
      const closeMatches = line.match(/<\/span>/g) || [];
      expect(openMatches.length).toBe(closeMatches.length);
      expect(line).toContain("hljs-comment");
    }
  });

  it("escapes HTML special characters in code", () => {
    // Lone CR line endings must split like splitPreviewLines, or source rows drift.
    expect(highlightLines("const a = 1;\rconst b = 2;", "typescript")).toHaveLength(2);
    const code = "const el = <div>text</div>;";
    const lines = highlightLines(code, "typescript");

    expect(lines).not.toBeNull();
    expect(lines).toHaveLength(1);
    const plain = lines![0].replace(/<\/?span[^>]*>/g, "");
    expect(plain).toContain("&lt;div&gt;");
    expect(plain).not.toContain("<div");
  });

  it("returns null when text length or line count exceeds limits", () => {
    const oversizeText = "a".repeat(512 * 1024 + 1);
    expect(highlightLines(oversizeText, "typescript")).toBeNull();

    const manyLines = "const x = 1;\n".repeat(20001);
    expect(highlightLines(manyLines, "typescript")).toBeNull();
  });
});
