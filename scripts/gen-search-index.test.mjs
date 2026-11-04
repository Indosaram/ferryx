import { test, expect } from "bun:test";
import { headings } from "./gen-search-index.mjs";

test("heading index preserves document identity and section bodies, ignoring fenced headings", () => {
  const index = headings("# Start\nhello\n```sh\n# not a heading\n```\n## Next ##\nworld", "docs/example.md");
  expect(index).toEqual([
    { path: "docs/example.md", title: "Start", text: "hello\n```sh\n# not a heading\n```\n" },
    { path: "docs/example.md", title: "Next", text: "world\n" },
  ]);
});
