import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SystemPermissionsStatus } from "../../lib/types";
import { TerminalSection } from "./TerminalSection";

const mockTauri = vi.hoisted(() => ({
  getSystemPermissionsStatus: vi.fn(),
}));

vi.mock("../../lib/tauri", async () => {
  const actual = await vi.importActual<typeof import("../../lib/tauri")>("../../lib/tauri");
  return {
    ...actual,
    getSystemPermissionsStatus: () => mockTauri.getSystemPermissionsStatus(),
  };
});

/**
 * Reports `platform` as the HOST platform and resolves once the component has actually
 * asked for it, so tests await the exact IPC request instead of a fixed delay.
 */
function stubHostPlatform(platform: string) {
  return new Promise<void>((resolve) => {
    mockTauri.getSystemPermissionsStatus.mockImplementation(() => {
      resolve();
      return Promise.resolve({ platform } as SystemPermissionsStatus);
    });
  });
}

beforeEach(() => {
  // Unresolved by default: tests that do not care about the host platform must not
  // receive a state update outside act().
  mockTauri.getSystemPermissionsStatus.mockImplementation(() => new Promise(() => {}));
});

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

  it("renders the scrollback compression toggle on by default and reports changes", () => {
    const onScrollbackCompression = vi.fn();
    const { rerender } = render(
      <TerminalSection {...defaultProps} onScrollbackCompression={onScrollbackCompression} />,
    );

    const toggle = screen.getByRole("switch", { name: /compress idle scrollback/i });
    expect(toggle).toHaveAttribute("aria-checked", "true");

    fireEvent.click(toggle);
    expect(onScrollbackCompression).toHaveBeenCalledExactlyOnceWith(false);

    rerender(
      <TerminalSection
        {...defaultProps}
        scrollbackCompression={false}
        onScrollbackCompression={onScrollbackCompression}
      />,
    );
    expect(screen.getByRole("switch", { name: /compress idle scrollback/i })).toHaveAttribute("aria-checked", "false");
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

  it("renders Default shell selector and changing it calls onShell", async () => {
    const ready = stubHostPlatform("macos");
    const onShell = vi.fn();
    render(<TerminalSection {...defaultProps} shell={null} onShell={onShell} />);
    await act(async () => {
      await ready;
    });

    const shellSelect = screen.getByLabelText(/default shell/i);
    expect(shellSelect).toBeInTheDocument();
    expect(shellSelect).toHaveValue("");

    fireEvent.change(shellSelect, { target: { value: "zsh" } });
    expect(onShell).toHaveBeenCalledWith("zsh");
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

describe("terminal platform applicability", () => {
  it.each(["Win32", "Linux x86_64", "MacIntel"])("gates Option control on %s", (platform) => {
    vi.stubGlobal("navigator", { platform, userAgent: platform });
    vi.stubGlobal("process", { ...process, platform: platform === "MacIntel" ? "darwin" : platform === "Win32" ? "win32" : "linux" });
    const onOptionAsAlt = vi.fn();
    const { container } = render(<TerminalSection
      fontFamily="monospace" fontSize={14} macosOptionAsAlt={false}
      source="default" sourcePath={null}
      onFontFamily={vi.fn()} onFontSize={vi.fn()} onOptionAsAlt={onOptionAsAlt}
      onUseImported={vi.fn()}
    />);
    const control = container.querySelector("#terminal-macos-option-as-alt");
    if (platform === "MacIntel") {
      expect(control).not.toBeNull();
      fireEvent.click(control!);
      expect(onOptionAsAlt).toHaveBeenCalledExactlyOnceWith(true);
    } else {
      expect(control).toBeNull();
      expect(onOptionAsAlt).not.toHaveBeenCalled();
    }
    expect(container.querySelector("#terminal-default-shell")).not.toBeNull();
  });
});

describe("default shell presets follow the host platform", () => {
  const hostProps = {
    fontFamily: "monospace",
    fontSize: 14,
    macosOptionAsAlt: false,
    source: "default",
    sourcePath: null,
    onFontFamily: vi.fn(),
    onFontSize: vi.fn(),
    onOptionAsAlt: vi.fn(),
    onUseImported: vi.fn(),
  };

  async function renderForHost(platform: string) {
    const ready = stubHostPlatform(platform);
    render(<TerminalSection {...hostProps} shell={null} onShell={vi.fn()} />);
    await act(async () => {
      await ready;
    });
  }

  function shellOptionValues(): string[] {
    const select = screen.getByLabelText(/default shell/i) as HTMLSelectElement;
    return Array.from(select.options).map((option) => option.value);
  }

  it.each(["macos", "linux"])("offers no Windows shells on a %s host", async (platform) => {
    await renderForHost(platform);

    const values = shellOptionValues();
    expect(values).toEqual(expect.arrayContaining(["", "zsh", "bash", "fish", "custom"]));
    expect(values).not.toContain("pwsh");
    expect(values).not.toContain("powershell");
    expect(values).not.toContain("cmd");
    expect(values).not.toContain("wsl");
  });

  it("offers only Windows shells on a Windows host", async () => {
    await renderForHost("windows");

    const values = shellOptionValues();
    expect(values).toEqual(expect.arrayContaining(["", "pwsh", "powershell", "cmd", "wsl", "custom"]));
    expect(values).not.toContain("zsh");
    expect(values).not.toContain("bash");
    expect(values).not.toContain("fish");
  });
});
