import { readFileSync } from "node:fs";
import { join } from "node:path";
import postcss from "postcss";
import { afterEach, describe, expect, it } from "vitest";

const stylesheet = postcss.parse(readFileSync(join(process.cwd(), "src/index.css"), "utf8"));
const nativeRules = stylesheet.nodes
  .filter((node) => node.type === "rule" && node.selector.includes("platform-macos"))
  .map((node) => node.toString()).join("\n");

afterEach(() => {
  document.documentElement.classList.remove("platform-macos");
  document.body.replaceChildren();
});

describe("native terminal backing ownership", () => {
  it("paints an unpresented pane without covering an adjacent live native pane", () => {
    document.documentElement.classList.add("platform-macos");
    const style = document.createElement("style");
    style.textContent = nativeRules;
    const cold = document.createElement("div");
    cold.setAttribute("data-testid", "native-terminal-pane");
    cold.setAttribute("data-native-terminal-presented", "false");
    const live = cold.cloneNode();
    if (!(live instanceof HTMLElement)) throw new Error("Expected an element");
    live.setAttribute("data-native-terminal-presented", "true");
    document.body.append(style, cold, live);

    expect(getComputedStyle(cold).backgroundColor).toBe("var(--terminal)");
    expect(getComputedStyle(live).backgroundColor).toBe("rgba(0, 0, 0, 0)");

    cold.setAttribute("data-native-terminal-presented", "true");
    expect(getComputedStyle(cold).backgroundColor).toBe("rgba(0, 0, 0, 0)");
  });
});
