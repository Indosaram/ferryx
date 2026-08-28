import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { TerminalSection } from "./TerminalSection";

const FONT_SIZE_MIN = 10;
const FONT_SIZE_MAX = 24;
const PERSISTED_FONT_FAMILY = "MesloLGS NF";

function clampLikeNormalizeTerminalSettings(value: number): number {
  return Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, Math.round(value)));
}

function ParentThatClampsAndFeedsBack({ onCommit }: { onCommit: (value: number) => void }) {
  const [fontSize, setFontSize] = useState(13);
  return (
    <TerminalSection
      fontFamily={PERSISTED_FONT_FAMILY}
      fontSize={fontSize}
      macosOptionAsAlt={false}
      source="imported"
      sourcePath={null}
      onFontFamily={vi.fn()}
      onFontSize={(value) => {
        onCommit(value);
        setFontSize(clampLikeNormalizeTerminalSettings(value));
      }}
      onOptionAsAlt={vi.fn()}
      onUseImported={vi.fn()}
    />
  );
}

describe("TerminalSection interaction safety", () => {
  it("lets the user type a multi-digit font size without the clamp overwriting the draft", () => {
    const onCommit = vi.fn();
    render(<ParentThatClampsAndFeedsBack onCommit={onCommit} />);

    const input = screen.getByLabelText(/font size/i) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.change(input, { target: { value: "1" } });

    expect(input.value).toBe("1");

    fireEvent.change(input, { target: { value: "14" } });
    fireEvent.blur(input);

    expect(input.value).toBe("14");
    expect(onCommit).toHaveBeenLastCalledWith(14);
  });

  it("does not commit an out-of-range intermediate value while typing", () => {
    const onCommit = vi.fn();
    render(<ParentThatClampsAndFeedsBack onCommit={onCommit} />);

    const input = screen.getByLabelText(/font size/i) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.change(input, { target: { value: "1" } });

    expect(onCommit).not.toHaveBeenCalledWith(1);
  });

  it("restores the persisted value when blurring a whitespace-only font family", () => {
    render(<ParentThatClampsAndFeedsBack onCommit={vi.fn()} />);

    const input = screen.getByLabelText(/font family/i) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.blur(input);

    expect(input.value).toBe(PERSISTED_FONT_FAMILY);
  });
});
