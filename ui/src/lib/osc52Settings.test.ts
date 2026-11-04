import { describe, expect, it } from "vitest";
import { loadTerminalSettings, saveTerminalSettings } from "./terminalSettings";

describe("terminal.osc52Write persistence", () => {
  it("defaults off and persists explicit on and off choices", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };
    expect(loadTerminalSettings(storage).osc52Write).toBe(false);
    saveTerminalSettings({ osc52Write: true }, storage);
    expect(loadTerminalSettings(storage).osc52Write).toBe(true);
    saveTerminalSettings({ osc52Write: false }, storage);
    expect(loadTerminalSettings(storage).osc52Write).toBe(false);
  });
});
