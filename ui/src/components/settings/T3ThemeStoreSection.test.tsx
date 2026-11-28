import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ThemeDocumentV1 } from "../../lib/themeDocument";

const store = vi.hoisted(() => ({
  load: vi.fn(),
  reset: vi.fn(),
}));

vi.mock("../../lib/t3ThemeStore", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/t3ThemeStore")>();
  return { ...actual, loadT3StoreCatalog: store.load };
});

import { T3ThemeStoreSection } from "./T3ThemeStoreSection";

function doc(id: string, name: string): ThemeDocumentV1 {
  return {
    version: 1,
    id,
    name,
    base: "dark",
    tokens: { "--background": "#101214" },
  } as ThemeDocumentV1;
}

function theme(id: string, name: string, author: string | null) {
  return {
    id,
    name,
    author,
    description: null,
    base: "dark" as const,
    addedAt: 0,
    swatches: ["#101214"],
    doc: doc(id, name),
  };
}

describe("T3ThemeStoreSection", () => {
  beforeEach(() => {
    store.load.mockReset();
    store.reset.mockReset();
  });

  afterEach(cleanup);

  it("loads the store on Browse and installs a theme on click", async () => {
    store.load.mockResolvedValue({
      ok: true,
      skipped: 0,
      themes: [theme("basalt-shore", "Basalt Shore", "ada"), theme("ember-ash", "Ember Ash", "bo")],
    });
    const onInstallThemes = vi.fn();
    render(<T3ThemeStoreSection onInstallThemes={onInstallThemes} />);

    fireEvent.click(screen.getByRole("button", { name: /browse/i }));

    expect(await screen.findByText("Basalt Shore")).toBeTruthy();
    expect(screen.getByText(/ada · dark/)).toBeTruthy();

    const rows = screen.getAllByRole("button", { name: /install/i });
    fireEvent.click(rows[1]);

    expect(onInstallThemes).toHaveBeenCalledTimes(1);
    const [docs] = onInstallThemes.mock.calls[0];
    expect(docs).toHaveLength(1);
    expect(docs[0].id).toBe("ember-ash");
    expect(await screen.findByText(/Installed Ember Ash/)).toBeTruthy();
  });

  it("filters the loaded catalog by query", async () => {
    store.load.mockResolvedValue({
      ok: true,
      skipped: 0,
      themes: [theme("basalt-shore", "Basalt Shore", "ada"), theme("ember-ash", "Ember Ash", "bo")],
    });
    render(<T3ThemeStoreSection onInstallThemes={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /browse/i }));
    await screen.findByText("Basalt Shore");

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "ember" } });

    await waitFor(() => expect(screen.queryByText("Basalt Shore")).toBeNull());
    expect(screen.getByText("Ember Ash")).toBeTruthy();
  });

  it("reports a load failure instead of an empty list", async () => {
    store.load.mockResolvedValue({
      ok: false,
      error: { code: "network", message: "Could not reach the t3code theme store." },
    });
    render(<T3ThemeStoreSection onInstallThemes={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /browse/i }));

    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      "Could not reach the t3code theme store.",
    );
  });

  it("surfaces how many store themes were skipped", async () => {
    store.load.mockResolvedValue({
      ok: true,
      skipped: 3,
      themes: [theme("basalt-shore", "Basalt Shore", "ada")],
    });
    render(<T3ThemeStoreSection onInstallThemes={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /browse/i }));

    expect(await screen.findByText(/1 themes; 3 could not be imported/)).toBeTruthy();
  });
});
