import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  setTerminalAppearance: vi.fn(),
  saveFile: vi.fn<(fileName: string, text: string) => Promise<string>>(),
  themeListeners: [] as Array<(event: { payload: string }) => void>,
}));

vi.mock("../../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/tauri")>();
  return { ...actual, setTerminalAppearance: native.setTerminalAppearance, saveSupervisorTranscriptFile: native.saveFile, isTauriRuntime: () => true };
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onThemeChanged: async (callback: (event: { payload: string }) => void) => {
      native.themeListeners.push(callback);
      return () => {
        const index = native.themeListeners.indexOf(callback);
        if (index >= 0) native.themeListeners.splice(index, 1);
      };
    },
  }),
}));

import {
  _resetNativeOsThemeForTest,
  loadAppearanceSettings,
  loadAppearanceSettingsState,
  applyAppearanceSettings,
  DEFAULT_APPEARANCE_SETTINGS,
  saveAppearanceSettings,
  useApplyAppearanceSettings,
} from "../../lib/appearanceSettings";
import {
  _resetSettingsRuntimeBridgeForTest,
  installSettingsRuntimeBridge,
} from "../../lib/settingsRuntimeBridge";
import { AppearanceSection } from "./AppearanceSection";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "../../lib/storageKeys";
import { parseThemeDocument, type ThemeDocumentV1 } from "../../lib/themeDocument";

type MediaStub = { matches: boolean; fire: () => void };

function stubColorScheme(prefersLight: boolean): MediaStub {
  const listeners: Array<() => void> = [];
  const stub: MediaStub = { matches: prefersLight, fire: () => listeners.forEach((listener) => listener()) };
  vi.stubGlobal("matchMedia", vi.fn().mockImplementation((query: string) => ({
    get matches() {
      return stub.matches;
    },
    media: query,
    addEventListener: (_event: string, cb: () => void) => listeners.push(cb),
    removeEventListener: (_event: string, cb: () => void) => {
      const index = listeners.indexOf(cb);
      if (index >= 0) listeners.splice(index, 1);
    },
  })));
  return stub;
}

/** What App renders: the hook that applies the theme, plus the settings section. */
function Harness() {
  useApplyAppearanceSettings();
  return <AppearanceSection />;
}

function fireOsTheme(payload: "light" | "dark") {
  act(() => {
    for (const listener of [...native.themeListeners]) listener({ payload });
  });
}

describe("AppearanceSection system theme", () => {
  beforeEach(() => {
    localStorage.clear();
    native.themeListeners.length = 0;
    native.setTerminalAppearance.mockReset();
    native.setTerminalAppearance.mockResolvedValue(null);
    _resetNativeOsThemeForTest();
    _resetSettingsRuntimeBridgeForTest();
    document.documentElement.removeAttribute("data-theme");
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
  });

  afterEach(() => {
    cleanup();
    _resetSettingsRuntimeBridgeForTest();
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.unstubAllGlobals();
  });

  it("offers a System option that persists and follows prefers-color-scheme", async () => {
    const media = stubColorScheme(true);
    render(<Harness />);

    const trigger = screen.getByRole("combobox", { name: "Theme mode" });
    fireEvent.pointerDown(trigger, { pointerId: 1, button: 0 });
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("option", { name: "System (follow OS)" }));

    expect(loadAppearanceSettings().theme).toBe("system");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.dataset.themePreference).toBe("system");
    expect(screen.getByText(/Currently Light/)).toBeInTheDocument();

    media.matches = false;
    act(() => media.fire());
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByText(/Currently Dark/)).toBeInTheDocument();
  });

  it("follows the native OS theme event even when the media query does not change", async () => {
    stubColorScheme(false);
    saveAppearanceSettings({ theme: "system" });
    render(<Harness />);
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(native.themeListeners.length).toBeGreaterThan(0);

    fireOsTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(screen.getByText(/Currently Light/)).toBeInTheDocument();

    fireOsTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByText(/Currently Dark/)).toBeInTheDocument();
  });

  it("switches the terminal to its light palette when the OS turns light", async () => {
    stubColorScheme(false);
    saveAppearanceSettings({ theme: "system" });
    installSettingsRuntimeBridge();
    expect(native.setTerminalAppearance).toHaveBeenLastCalledWith("dark");
    expect(native.themeListeners.length).toBeGreaterThan(0);

    fireOsTheme("light");
    expect(native.setTerminalAppearance).toHaveBeenLastCalledWith("light");
  });

  it("ignores OS theme events while a fixed theme is chosen", async () => {
    stubColorScheme(false);
    saveAppearanceSettings({ theme: "charcoal" });
    installSettingsRuntimeBridge();
    render(<Harness />);
    expect(native.themeListeners.length).toBeGreaterThan(0);

    fireOsTheme("light");
    expect(document.documentElement.dataset.theme).toBe("charcoal");
    expect(native.setTerminalAppearance).not.toHaveBeenCalledWith("light");
  });
});

const themeFixture: ThemeDocumentV1 = {
  version: 1, id: "ocean-night", name: "Ocean night", base: "light",
  tokens: { "--primary": "#123456" }, css: ".sample { color: #123456; }",
};

function pasteTheme(raw: unknown) {
  fireEvent.change(screen.getByLabelText("Theme JSON"), { target: { value: JSON.stringify(raw) } });
  fireEvent.click(screen.getByRole("button", { name: "Import pasted JSON" }));
}

// jsdom versions without Blob.text still exercise the real file-input/read/parse path.
async function chooseThemeFile(text: string) {
  const file = new File([text], "sample.ferryx-theme.json", { type: "application/json" });
  let finishRead!: (text: string) => void;
  const read = new Promise<string>((resolve) => { finishRead = resolve; });
  Object.defineProperty(file, "text", { value: () => read });
  fireEvent.change(screen.getByLabelText("Import theme file"), { target: { files: [file] } });
  await act(async () => { finishRead(text); await read; });
}

describe("AppearanceSection theme library", () => {
  beforeEach(() => {
    localStorage.clear();
    stubColorScheme(false);
    native.saveFile.mockReset();
    native.saveFile.mockResolvedValue("Downloads/theme.ferryx-theme.json");
    native.themeListeners.length = 0;
    _resetNativeOsThemeForTest();
  });

  afterEach(() => {
    cleanup();
    applyAppearanceSettings(DEFAULT_APPEARANCE_SETTINGS);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("imports a valid file, adds its row, and activates its base and tokens", async () => {
    render(<Harness />);
    await chooseThemeFile(JSON.stringify(themeFixture));
    expect(loadAppearanceSettings().customThemes).toEqual([themeFixture]);
    expect(loadAppearanceSettings().activeCustomThemeId).toBe(themeFixture.id);
    expect(screen.getByRole("button", { name: "Use Ocean night" })).toHaveAttribute("aria-pressed", "true");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.style.getPropertyValue("--primary")).toBe("#123456");
  });

  it("rejects a bad file atomically and reports every bad field", async () => {
    saveAppearanceSettings({ customThemes: [themeFixture], activeCustomThemeId: themeFixture.id });
    render(<Harness />);
    const before = loadAppearanceSettings();
    await chooseThemeFile(JSON.stringify({ version: 1, id: "X", name: "", base: "sepia", tokens: { "--nope": "#fff", "--radius": "2px" } }));
    expect(loadAppearanceSettings()).toEqual(before);
    const rows = within(screen.getByRole("alert")).getAllByRole("listitem");
    expect(rows).toHaveLength(5);
    for (const path of ["id", "name", "base", "tokens.--nope", "tokens.--radius"]) {
      expect(rows.some((row) => row.textContent?.startsWith(`${path}:`))).toBe(true);
    }
  });

  it("exports and reimports the identical active document without standalone CSS", async () => {
    saveAppearanceSettings({ customThemes: [themeFixture], activeCustomThemeId: themeFixture.id, customCss: ".sample { color: #abcdef; }" });
    render(<Harness />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Export active theme" })); });
    expect(native.saveFile).toHaveBeenCalledWith("ocean-night.ferryx-theme.json", JSON.stringify(themeFixture, null, 2));
    const exported = native.saveFile.mock.calls[0][1];
    fireEvent.click(screen.getByRole("button", { name: "Delete Ocean night" }));
    await chooseThemeFile(exported);
    expect(loadAppearanceSettings().customThemes).toEqual([themeFixture]);
    expect(loadAppearanceSettings().customCss).toBe(".sample { color: #abcdef; }");
  });

  it("renames, reselects and deletes a theme, restoring the built-in palette", () => {
    saveAppearanceSettings({ customThemes: [themeFixture], activeCustomThemeId: themeFixture.id });
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Rename Ocean night" }));
    fireEvent.change(screen.getByLabelText("Name for Ocean night"), { target: { value: "Renamed ocean" } });
    fireEvent.click(screen.getByRole("button", { name: "Save name" }));
    expect(loadAppearanceSettings().customThemes[0]).toEqual({ ...themeFixture, name: "Renamed ocean" });
    fireEvent.click(screen.getByRole("button", { name: "Use charcoal" }));
    expect(loadAppearanceSettings().activeCustomThemeId).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Use Renamed ocean" }));
    expect(document.documentElement.dataset.theme).toBe("light");
    fireEvent.click(screen.getByRole("button", { name: "Delete Renamed ocean" }));
    expect(loadAppearanceSettings().activeCustomThemeId).toBeNull();
    expect(loadAppearanceSettings().customThemes).toEqual([]);
    expect(document.documentElement.dataset.theme).toBe("charcoal");
    expect(document.documentElement.style.getPropertyValue("--primary")).toBe("");
  });

  it("keeps a corrupted stored document and its diagnostics visible after settings writes", () => {
    const raw = { ...themeFixture, base: "sepia" };
    localStorage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify({ customThemes: [raw], activeCustomThemeId: raw.id }));
    render(<Harness />);
    expect(document.documentElement.dataset.theme).toBe("charcoal");
    expect(screen.getByRole("alert")).toHaveTextContent("base:");
    expect(loadAppearanceSettingsState().invalidThemes[0].raw).toEqual(raw);
    fireEvent.click(screen.getByRole("checkbox", { name: "Render math in chat" }));
    expect(loadAppearanceSettings().renderMath).toBe(false);
    expect(loadAppearanceSettingsState().invalidThemes[0].raw).toEqual(raw);
    expect(screen.getByRole("alert")).toHaveTextContent("base:");
  });

  it("authors a JSON draft from current accent/base tokens without saving until import", () => {
    saveAppearanceSettings({ theme: "light", accentColor: "rose" });
    render(<Harness />);
    const computed = getComputedStyle(document.documentElement);
    vi.spyOn(window, "getComputedStyle").mockReturnValue({
      getPropertyValue: (token: string) => token === "--primary" ? "#be123c" : computed.getPropertyValue(token),
    } as CSSStyleDeclaration);
    fireEvent.click(screen.getByRole("button", { name: "New theme from current appearance" }));
    const draft = JSON.parse((screen.getByLabelText("Theme JSON") as HTMLTextAreaElement).value);
    expect(parseThemeDocument(draft).ok).toBe(true);
    expect(draft.base).toBe("light");
    expect(draft.tokens["--primary"]).toBe("#be123c");
    expect(loadAppearanceSettings().customThemes).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Import pasted JSON" }));
    expect(loadAppearanceSettings().customThemes).toEqual([draft]);
  });

  it.each([
    ["empty document", {}, "version"],
    ["oversized CSS", { ...themeFixture, css: "x".repeat(40 * 1024) }, "css"],
    ["unsafe theme CSS", { ...themeFixture, css: ".sample { position: fixed; }" }, "theme.css.rule 0"],
    ["unreadable colours", { ...themeFixture, tokens: { "--chat-foreground": "#777", "--chat-surface": "#888" } }, "tokens.--chat-foreground"],
  ])("refuses %s without adding a theme", (_label, raw, path) => {
    render(<Harness />);
    pasteTheme(raw);
    expect(loadAppearanceSettings().customThemes).toEqual([]);
    expect(screen.getByRole("alert")).toHaveTextContent(`${path}:`);
  });

  it("refuses unsafe standalone CSS per rule without changing stored CSS", () => {
    saveAppearanceSettings({ customCss: ".sample { color: #123456; }" });
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Custom CSS..." }));
    fireEvent.change(screen.getByLabelText("Standalone custom CSS (applied last)"), { target: { value: ".sample { position: fixed; }" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply custom CSS" }));
    expect(screen.getByRole("alert")).toHaveTextContent("customCss.rule 0:");
    expect(loadAppearanceSettings().customCss).toBe(".sample { color: #123456; }");
  });

  it("applies standalone CSS after document CSS, exports it separately, and removes it outside chat", async () => {
    saveAppearanceSettings({ customThemes: [themeFixture], activeCustomThemeId: themeFixture.id });
    render(<><Harness /><div data-chat-scope=""><span className="sample" data-testid="css-sample">Sample</span></div></>);
    fireEvent.click(screen.getByRole("button", { name: "Custom CSS..." }));
    fireEvent.change(screen.getByLabelText("Standalone custom CSS (applied last)"), { target: { value: ".sample { color: #abcdef; }" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply custom CSS" }));
    expect(getComputedStyle(screen.getByTestId("css-sample")).color).toBe("rgb(171, 205, 239)");
    const sheets = [...document.querySelectorAll("style[data-custom-theme-css]")];
    expect(sheets.map((sheet) => sheet.textContent)).toEqual([
      expect.stringContaining("#123456"), expect.stringContaining("#abcdef"),
    ]);
    const recovery = screen.getByRole("button", { name: "Remove custom CSS" });
    expect(recovery.closest("[data-chat-scope]")).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Export custom CSS" })); });
    expect(native.saveFile).toHaveBeenCalledWith("custom.ferryx.css", ".sample { color: #abcdef; }");
    fireEvent.click(recovery);
    expect(loadAppearanceSettings().customCss).toBe("");
    expect(getComputedStyle(screen.getByTestId("css-sample")).color).toBe("rgb(18, 52, 86)");
  });
});
