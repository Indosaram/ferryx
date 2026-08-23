import { beforeEach, describe, expect, it } from "vitest";

import * as storageKeys from "./storageKeys";
import {
  ACTIVE_PROJECT_STORAGE_KEY,
  GENERAL_SETTINGS_STORAGE_KEY,
  getMigratedItem,
  NOTIFICATION_SETTINGS_STORAGE_KEY,
  PROJECTS_STORAGE_KEY,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_OPEN_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  TERMINAL_SETTINGS_STORAGE_KEY,
} from "./storageKeys";

describe("storageKeys unification and migration", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("all storage keys start with ferryx.", () => {
    const keys = [
      PROJECTS_STORAGE_KEY,
      ACTIVE_PROJECT_STORAGE_KEY,
      SIDEBAR_OPEN_STORAGE_KEY,
      SIDEBAR_WIDTH_STORAGE_KEY,
      SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
      TERMINAL_SETTINGS_STORAGE_KEY,
      NOTIFICATION_SETTINGS_STORAGE_KEY,
      GENERAL_SETTINGS_STORAGE_KEY,
    ];
    for (const key of keys) {
      expect(key.startsWith("ferryx.")).toBe(true);
    }
  });

  it("does not expose a dedicated Quick Commands storage key", () => {
    expect("QUICK_COMMANDS_STORAGE_KEY" in storageKeys).toBe(false);
    const stringValues = Object.values(storageKeys).filter((value) => typeof value === "string");
    expect(stringValues).not.toContain("ferryx.settings.quickCommands");
  });

  it("returns current value when new key exists", () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "proj-1" }]));
    localStorage.setItem("rorca.projects", JSON.stringify([{ workspaceId: "old-proj" }]));

    const result = getMigratedItem(PROJECTS_STORAGE_KEY, localStorage);
    expect(result).toBe(JSON.stringify([{ workspaceId: "proj-1" }]));
  });

  it("migrates legacy key once and persists to new key when new key is absent (F10)", () => {
    localStorage.setItem("rorca.projects", JSON.stringify([{ workspaceId: "legacy-proj" }]));
    expect(localStorage.getItem(PROJECTS_STORAGE_KEY)).toBeNull();

    const result = getMigratedItem(PROJECTS_STORAGE_KEY, localStorage);
    expect(result).toBe(JSON.stringify([{ workspaceId: "legacy-proj" }]));
    expect(localStorage.getItem(PROJECTS_STORAGE_KEY)).toBe(JSON.stringify([{ workspaceId: "legacy-proj" }]));
  });

  it("migrates each defined key from its legacy counterparts", () => {
    localStorage.setItem("orca.sidebar.width", "350");
    expect(getMigratedItem(SIDEBAR_WIDTH_STORAGE_KEY, localStorage)).toBe("350");
    expect(localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY)).toBe("350");

    localStorage.setItem("orca.terminal.settings", JSON.stringify({ fontSize: 16 }));
    expect(getMigratedItem(TERMINAL_SETTINGS_STORAGE_KEY, localStorage)).toBe(JSON.stringify({ fontSize: 16 }));
    expect(localStorage.getItem(TERMINAL_SETTINGS_STORAGE_KEY)).toBe(JSON.stringify({ fontSize: 16 }));

    localStorage.setItem("rorca:settings:notifications:v1", JSON.stringify({ enabled: false }));
    expect(getMigratedItem(NOTIFICATION_SETTINGS_STORAGE_KEY, localStorage)).toBe(JSON.stringify({ enabled: false }));
    expect(localStorage.getItem(NOTIFICATION_SETTINGS_STORAGE_KEY)).toBe(JSON.stringify({ enabled: false }));

    localStorage.setItem("orca.sidebar.open", "false");
    expect(getMigratedItem(SIDEBAR_OPEN_STORAGE_KEY, localStorage)).toBe("false");
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("false");

    localStorage.setItem("rorca.active-project", "project-x");
    expect(getMigratedItem(ACTIVE_PROJECT_STORAGE_KEY, localStorage)).toBe("project-x");
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("project-x");

    localStorage.setItem("rorca.sidebar.collapsedProjects", JSON.stringify(["p1"]));
    expect(getMigratedItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, localStorage)).toBe(JSON.stringify(["p1"]));
    expect(localStorage.getItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY)).toBe(JSON.stringify(["p1"]));

    localStorage.setItem("rorca.settings.general", JSON.stringify({ confirmCloseTab: true }));
    expect(getMigratedItem(GENERAL_SETTINGS_STORAGE_KEY, localStorage)).toBe(JSON.stringify({ confirmCloseTab: true }));
    expect(localStorage.getItem(GENERAL_SETTINGS_STORAGE_KEY)).toBe(JSON.stringify({ confirmCloseTab: true }));

    localStorage.removeItem(GENERAL_SETTINGS_STORAGE_KEY);
    localStorage.setItem("orca.settings.general", JSON.stringify({ confirmCloseTab: true }));
    expect(getMigratedItem(GENERAL_SETTINGS_STORAGE_KEY, localStorage)).toBe(JSON.stringify({ confirmCloseTab: true }));
    expect(localStorage.getItem(GENERAL_SETTINGS_STORAGE_KEY)).toBe(JSON.stringify({ confirmCloseTab: true }));
  });
});
