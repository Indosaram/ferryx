import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TerminalSection } from "./TerminalSection";

afterEach(cleanup);

describe("TerminalSection", () => {
  const defaultProps = {
    fontFamily: "JetBrains Mono",
    fontSize: 14,
    macosOptionAsAlt: true,
    source: "Ghostty · Imported",
    sourcePath: "/Users/test/.config/ghostty/config",
    onFontFamily: vi.fn(),
    onFontSize: vi.fn(),
    onOptionAsAlt: vi.fn(),
    onUseImported: vi.fn(),
  };

  it("renders Scrollback control and commits changes on blur or Enter", () => {
    const onScrollback = vi.fn();
    render(<TerminalSection {...defaultProps} scrollback={10_000} onScrollback={onScrollback} />);

    const scrollbackInput = screen.getByLabelText(/scrollback lines/i);
    expect(scrollbackInput).toBeInTheDocument();
    expect(scrollbackInput).toHaveValue(10_000);

    fireEvent.change(scrollbackInput, { target: { value: "25000" } });
    expect(onScrollback).not.toHaveBeenCalled();

    fireEvent.blur(scrollbackInput);
    expect(onScrollback).toHaveBeenCalledWith(25_000);

    fireEvent.change(scrollbackInput, { target: { value: "50000" } });
    fireEvent.keyDown(scrollbackInput, { key: "Enter" });
    expect(onScrollback).toHaveBeenCalledWith(50_000);

    // Clamps out of range numbers
    fireEvent.change(scrollbackInput, { target: { value: "200000" } });
    fireEvent.blur(scrollbackInput);
    expect(onScrollback).toHaveBeenCalledWith(100_000);
  });

  it("buffers font family changes and commits only on blur or Enter", () => {
    const onFontFamily = vi.fn();
    render(<TerminalSection {...defaultProps} onFontFamily={onFontFamily} />);

    const fontInput = screen.getByLabelText(/font family/i);
    fireEvent.change(fontInput, { target: { value: "F" } });
    fireEvent.change(fontInput, { target: { value: "Fi" } });
    fireEvent.change(fontInput, { target: { value: "Fir" } });
    fireEvent.change(fontInput, { target: { value: "Fira" } });
    fireEvent.change(fontInput, { target: { value: "Fira Code" } });

    expect(onFontFamily).toHaveBeenCalledTimes(0);

    fireEvent.blur(fontInput);
    expect(onFontFamily).toHaveBeenCalledTimes(1);
    expect(onFontFamily).toHaveBeenCalledWith("Fira Code");

    fireEvent.change(fontInput, { target: { value: "Menlo" } });
    expect(onFontFamily).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(fontInput, { key: "Enter" });
    expect(onFontFamily).toHaveBeenCalledTimes(2);
    expect(onFontFamily).toHaveBeenLastCalledWith("Menlo");
  });

  it("does not call onFontSize with 0 or NaN when clearing the input", () => {
    const onFontSize = vi.fn();
    render(<TerminalSection {...defaultProps} fontSize={14} onFontSize={onFontSize} />);

    const fontSizeInput = screen.getByLabelText(/font size/i);
    fireEvent.change(fontSizeInput, { target: { value: "" } });

    expect(onFontSize).not.toHaveBeenCalled();

    fireEvent.blur(fontSizeInput);
    expect(fontSizeInput).toHaveValue(14);
  });

  it("restores draft font size from prop on blur when unparseable", () => {
    const onFontSize = vi.fn();
    render(<TerminalSection {...defaultProps} fontSize={16} onFontSize={onFontSize} />);

    const fontSizeInput = screen.getByLabelText(/font size/i);
    fireEvent.change(fontSizeInput, { target: { value: "   " } });
    expect(onFontSize).not.toHaveBeenCalled();

    fireEvent.blur(fontSizeInput);
    expect(fontSizeInput).toHaveValue(16);
  });

  it("updates font family and font size drafts when props change", () => {
    const { rerender } = render(<TerminalSection {...defaultProps} fontFamily="Monaco" fontSize={12} />);

    expect(screen.getByLabelText(/font family/i)).toHaveValue("Monaco");
    expect(screen.getByLabelText(/font size/i)).toHaveValue(12);

    rerender(<TerminalSection {...defaultProps} fontFamily="Courier" fontSize={18} />);

    expect(screen.getByLabelText(/font family/i)).toHaveValue("Courier");
    expect(screen.getByLabelText(/font size/i)).toHaveValue(18);
  });

  it("renders Default shell selector and changing it calls onShell", () => {
    const onShell = vi.fn();
    render(<TerminalSection {...defaultProps} shell={null} onShell={onShell} />);

    const shellSelect = screen.getByLabelText(/default shell/i);
    expect(shellSelect).toBeInTheDocument();
    expect(shellSelect).toHaveValue("");

    fireEvent.change(shellSelect, { target: { value: "pwsh" } });
    expect(onShell).toHaveBeenCalledWith("pwsh");
  });

  it("handles custom shell path input and commits on blur/enter", () => {
    const onShell = vi.fn();
    render(<TerminalSection {...defaultProps} shell={null} onShell={onShell} />);

    const shellSelect = screen.getByLabelText(/default shell/i);
    fireEvent.change(shellSelect, { target: { value: "custom" } });

    const customInput = screen.getByLabelText(/custom shell path/i);
    expect(customInput).toBeInTheDocument();

    fireEvent.change(customInput, { target: { value: "/bin/fish" } });
    fireEvent.blur(customInput);
    expect(onShell).toHaveBeenCalledWith("/bin/fish");
  });

  it("renders custom shell selection when prop is custom path", () => {
    render(<TerminalSection {...defaultProps} shell="/opt/homebrew/bin/nu" />);

    const shellSelect = screen.getByLabelText(/default shell/i);
    expect(shellSelect).toHaveValue("custom");

    const customInput = screen.getByLabelText(/custom shell path/i);
    expect(customInput).toHaveValue("/opt/homebrew/bin/nu");
  });
});
