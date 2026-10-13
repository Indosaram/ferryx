import "./previewHighlight.css";

import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import php from "highlight.js/lib/languages/php";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("python", python);
hljs.registerLanguage("go", go);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("css", css);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("java", java);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("csharp", csharp);
hljs.registerLanguage("ruby", ruby);
hljs.registerLanguage("php", php);
hljs.registerLanguage("swift", swift);
hljs.registerLanguage("kotlin", kotlin);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("diff", diff);
hljs.registerLanguage("dockerfile", dockerfile);

const MAX_HIGHLIGHT_BYTES = 512 * 1024;
const MAX_HIGHLIGHT_LINES = 20000;

const BASH_BASENAMES = new Set([
  ".bashrc",
  ".bash_profile",
  ".zshrc",
  ".zprofile",
  ".profile",
]);

export function languageForPath(name: string): string | null {
  const normalized = name.replace(/\\/g, "/");
  const basename = normalized.split("/").pop() ?? "";
  if (basename.toLowerCase() === "dockerfile" || basename.startsWith("Dockerfile.")) {
    return "dockerfile";
  }

  if (BASH_BASENAMES.has(basename.toLowerCase())) {
    return "bash";
  }

  const dotIndex = basename.lastIndexOf(".");
  if (dotIndex < 0 || dotIndex === basename.length - 1) {
    return null;
  }

  const ext = basename.slice(dotIndex + 1).toLowerCase();
  switch (ext) {
    case "ts":
    case "tsx":
    case "mts":
    case "cts":
      return "typescript";
    case "js":
    case "jsx":
    case "mjs":
    case "cjs":
      return "javascript";
    case "json":
    case "jsonc":
      return "json";
    case "rs":
      return "rust";
    case "py":
      return "python";
    case "go":
      return "go";
    case "sh":
    case "bash":
    case "zsh":
      return "bash";
    case "yml":
    case "yaml":
      return "yaml";
    case "css":
    case "scss":
      return "css";
    case "html":
    case "htm":
    case "xml":
    case "svg":
    case "plist":
      return "xml";
    case "sql":
      return "sql";
    case "java":
      return "java";
    case "c":
    case "h":
      return "c";
    case "cc":
    case "cpp":
    case "cxx":
    case "hpp":
    case "hh":
      return "cpp";
    case "cs":
      return "csharp";
    case "rb":
      return "ruby";
    case "php":
      return "php";
    case "swift":
      return "swift";
    case "kt":
    case "kts":
      return "kotlin";
    case "toml":
    case "ini":
    case "cfg":
      return "ini";
    case "diff":
    case "patch":
      return "diff";
    default:
      return null;
  }
}

export function highlightLines(text: string, language: string): string[] | null {
  if (text.length > MAX_HIGHLIGHT_BYTES) {
    return null;
  }

  if (!hljs.getLanguage(language)) {
    return null;
  }

  let highlighted = "";
  try {
    highlighted = hljs.highlight(text, { language, ignoreIllegals: true }).value;
  } catch {
    return null;
  }

  // Same line endings as splitPreviewLines (CRLF, lone CR, LF) so highlighted rows stay aligned.
  const rawLines = highlighted.split(/\r\n|\r|\n/);
  if (rawLines.length > MAX_HIGHLIGHT_LINES) {
    return null;
  }

  const tagPattern = /<\/?span\b[^>]*>/g;
  const result: string[] = [];
  const openStack: string[] = [];

  for (let i = 0; i < rawLines.length; i++) {
    const rawLine = rawLines[i]!;
    let lineOut = "";
    if (openStack.length > 0) {
      lineOut += openStack.join("");
    }

    let lastIndex = 0;
    tagPattern.lastIndex = 0;
    let match: RegExpExecArray | null;

    while ((match = tagPattern.exec(rawLine)) !== null) {
      lineOut += rawLine.slice(lastIndex, match.index);
      const tag = match[0];
      lineOut += tag;
      lastIndex = tagPattern.lastIndex;

      if (tag.startsWith("</")) {
        openStack.pop();
      } else {
        openStack.push(tag);
      }
    }

    lineOut += rawLine.slice(lastIndex);

    if (openStack.length > 0) {
      for (let j = openStack.length - 1; j >= 0; j--) {
        lineOut += "</span>";
      }
    }

    result.push(lineOut);
  }

  return result;
}
