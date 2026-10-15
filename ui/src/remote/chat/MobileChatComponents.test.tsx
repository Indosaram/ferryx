import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { ReferencePartList } from "./MobileChatComponents";

afterEach(cleanup);

describe("ReferencePartList prose lists", () => {
  it("keeps list markers on ordered and unordered lists in native turn markdown", () => {
    const { container } = render(
      <ReferencePartList
        parts={[
          { kind: "text", text: "Steps:\n\n1. Stop the daemon\n2. Rebuild\n3. Restart\n\nNotes:\n\n- one\n- two" },
        ]}
      />,
    );
    const ol = container.querySelector("ol");
    const ul = container.querySelector("ul");
    expect(ol).not.toBeNull();
    expect(ul).not.toBeNull();
    expect(ol!.className).toContain("list-decimal");
    expect(ol!.className).toContain("pl-4");
    expect(ul!.className).toContain("list-disc");
    expect(ul!.className).toContain("pl-4");
  });
});
