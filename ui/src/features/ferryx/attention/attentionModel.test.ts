import { describe, expect, it } from "vitest";

import { formatAttentionTime } from "./attentionModel";

describe("formatting", () => {
  it("describes elapsed time", () => {
    const now = 10 * 24 * 60 * 60 * 1000;
    expect(formatAttentionTime(now - 30_000, now)).toBe("just now");
    expect(formatAttentionTime(now - 2 * 60_000, now)).toBe("2m ago");
    expect(formatAttentionTime(now - 3 * 60 * 60_000, now)).toBe("3h ago");
    expect(formatAttentionTime(now - 2 * 24 * 60 * 60_000, now)).toBe("2d ago");
    expect(formatAttentionTime(now + 5_000, now)).toBe("just now");
  });
});
