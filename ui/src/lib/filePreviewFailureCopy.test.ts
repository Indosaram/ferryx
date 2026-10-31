import { describe, expect, it } from "vitest";
import { failureHeadline, previewServiceFailureHeadline } from "./filePreviewFailureCopy";

describe("previewServiceFailureHeadline", () => {
  it("maps a missing preview service to restart copy", () => {
    expect(
      previewServiceFailureHeadline({ code: "UNSUPPORTED", message: "x", details: { reason: "previewServiceUnavailable" } }),
    ).toMatch(/preview service could not start/);
  });

  it("leaves file-level and unknown reasons to failureHeadline", () => {
    expect(previewServiceFailureHeadline({ details: { reason: "TooLarge" } })).toBeNull();
    expect(previewServiceFailureHeadline(new Error("boom"))).toBeNull();
    expect(previewServiceFailureHeadline(null)).toBeNull();
    expect(failureHeadline("TooLarge")).toBe("This file is too large to preview.");
  });
});
