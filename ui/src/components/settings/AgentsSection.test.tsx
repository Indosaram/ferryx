import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AgentsSection } from "./AgentsSection";

const native = vi.hoisted(() => ({
  detectAgents: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({
  detectAgents: native.detectAgents,
}));

describe("AgentsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("surfaces agent detection failure with an alert message", async () => {
    native.detectAgents.mockRejectedValue(new Error("detect boom"));

    render(<AgentsSection />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("detect boom");
  });
});
