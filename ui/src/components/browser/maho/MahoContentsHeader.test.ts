import { describe, expect, it } from "vitest";

import { MAHO_SEARCH_PLACEHOLDER, mahoHeaderHostText } from "./MahoContentsHeader";

describe("mahoHeaderHostText", () => {
  it("drops a leading www. from the host", () => {
    expect(mahoHeaderHostText("https://www.github.com/ferryx")).toBe("github.com");
    expect(mahoHeaderHostText("https://docs.github.com/a?b=1")).toBe("docs.github.com");
  });

  it("falls back to the search placeholder when the URL has no host", () => {
    expect(mahoHeaderHostText(null)).toBe(MAHO_SEARCH_PLACEHOLDER);
    expect(mahoHeaderHostText("about:blank")).toBe(MAHO_SEARCH_PLACEHOLDER);
    expect(mahoHeaderHostText("file:///tmp/a.html")).toBe(MAHO_SEARCH_PLACEHOLDER);
    expect(mahoHeaderHostText("not a url")).toBe(MAHO_SEARCH_PLACEHOLDER);
  });
});
