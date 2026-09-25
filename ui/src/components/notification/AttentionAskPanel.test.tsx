import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentHistoryEntry, AgentHistoryMessage, AgentHistoryPage } from "../../lib/agentHistory";
import {
  AttentionAskPanel,
  collectAskEvidence,
  isWithinRange,
  type AttentionAskServices,
} from "./AttentionAskPanel";

afterEach(cleanup);

const NOW = 1_800_000_000_000;

function entry(overrides: Partial<AgentHistoryEntry> & Pick<AgentHistoryEntry, "entryKey">): AgentHistoryEntry {
  return {
    provider: "claude",
    providerSession: { key: "session_id", id: `session-${overrides.entryKey}`, transcriptPath: null },
    cwd: "/repo",
    version: null,
    parentId: null,
    modifiedMs: NOW - 60_000,
    ...overrides,
  };
}

function message(text: string): AgentHistoryMessage {
  return { ordinal: 1, role: "user", text, id: "m-1", parentId: null };
}

function services(overrides: Partial<AttentionAskServices> = {}): AttentionAskServices {
  return {
    search: vi.fn(async () => ({ items: [], nextCursor: null, partial: false, warnings: [] })),
    read: vi.fn(async () => ({ items: [], nextCursor: null, partial: false, warnings: [] })),
    ...overrides,
  };
}

describe("isWithinRange", () => {
  it("keeps everything for the all range", () => {
    expect(isWithinRange(entry({ entryKey: "a", modifiedMs: 1 }), { id: "all", label: "전체", ms: null }, NOW)).toBe(true);
  });

  it("drops entries older than the range", () => {
    const threeHours = { id: "3h", label: "3시간", ms: 3 * 60 * 60 * 1000 };
    expect(isWithinRange(entry({ entryKey: "a", modifiedMs: NOW - 60_000 }), threeHours, NOW)).toBe(true);
    expect(isWithinRange(entry({ entryKey: "b", modifiedMs: NOW - 4 * 60 * 60 * 1000 }), threeHours, NOW)).toBe(false);
  });

  it("keeps an entry whose age is unknown rather than hiding it", () => {
    expect(isWithinRange(entry({ entryKey: "a", modifiedMs: null }), { id: "15m", label: "15분", ms: 900_000 }, NOW)).toBe(true);
  });
});

describe("collectAskEvidence", () => {
  const threeHours = { id: "3h", label: "3시간", ms: 3 * 60 * 60 * 1000 };

  it("queries both providers and keeps only in-range entries", async () => {
    const search = vi.fn(async (request: { provider: string }) => ({
      items: [
        entry({ entryKey: `${request.provider}-recent`, provider: request.provider as "claude", modifiedMs: NOW - 1000 }),
        entry({ entryKey: `${request.provider}-stale`, provider: request.provider as "claude", modifiedMs: NOW - 9 * 60 * 60 * 1000 }),
      ],
      nextCursor: null,
      partial: false,
      warnings: [],
    }));
    const read = vi.fn(async (request: { entryKey: string }) => ({
      items: [message(`about ${request.entryKey}`)],
      nextCursor: null,
      partial: false,
      warnings: [],
    }));

    const result = await collectAskEvidence("about", threeHours, services({ search, read }), NOW);

    expect(search).toHaveBeenCalledTimes(2);
    expect(result.evidence.map((item) => item.entry.entryKey).sort()).toEqual(["claude-recent", "codex-recent"]);
  });

  it("keeps an entry only when a message actually matches the question", async () => {
    const search = vi.fn(async (request: { provider: string }) => ({
      items:
        request.provider === "claude"
          ? [entry({ entryKey: "match" }), entry({ entryKey: "nomatch" })]
          : [],
      nextCursor: null,
      partial: false,
      warnings: [],
    }));
    const read = vi.fn(async (request: { entryKey: string }) => ({
      items: [message(request.entryKey === "match" ? "we discussed the router" : "unrelated chatter")],
      nextCursor: null,
      partial: false,
      warnings: [],
    }));

    const result = await collectAskEvidence("router", threeHours, services({ search, read }), NOW);

    expect(result.evidence).toHaveLength(1);
    expect(result.evidence[0].entry.entryKey).toBe("match");
    expect(result.evidence[0].snippets).toEqual(["we discussed the router"]);
  });

  it("reports partial coverage instead of claiming a complete scan", async () => {
    const search = vi.fn(async () => ({ items: [], nextCursor: null, partial: true, warnings: ["SCAN_LIMIT"] }));
    const result = await collectAskEvidence("anything", threeHours, services({ search }), NOW);

    expect(result.partial).toBe(true);
    expect(result.warnings).toContain("SCAN_LIMIT");
  });
});

describe("AttentionAskPanel", () => {
  it("defaults to the three hour range", () => {
    render(<AttentionAskPanel services={services()} />);

    expect(screen.getByTestId("ask-range-3h").getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByTestId("ask-range-15m").getAttribute("aria-pressed")).toBe("false");
  });

  it("says there is no evidence when nothing matches in range", async () => {
    render(<AttentionAskPanel services={services()} />);

    await act(async () => {
      fireEvent.change(screen.getByLabelText("Ask past conversations"), { target: { value: "카페24 라우터" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "질문" }));
    });

    expect(screen.getByRole("status").textContent).toContain("답변할 근거가 없습니다");
    expect(screen.getByRole("status").textContent).toContain("카페24 라우터");
  });

  it("renders matched snippets as evidence", async () => {
    const search = vi.fn(async (request: { provider: string }) => ({
      items: request.provider === "claude" ? [entry({ entryKey: "hit" })] : [],
      nextCursor: null,
      partial: false,
      warnings: [],
    }));
    const read = vi.fn(async () => ({
      items: [message("라우터 얘기는 여기서 나왔습니다")],
      nextCursor: null,
      partial: false,
      warnings: [],
    }));

    render(<AttentionAskPanel services={services({ search, read })} />);

    await act(async () => {
      fireEvent.change(screen.getByLabelText("Ask past conversations"), { target: { value: "라우터" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "질문" }));
    });

    expect(screen.getByTestId("ask-evidence-hit")).toBeTruthy();
    expect(screen.getByText("라우터 얘기는 여기서 나왔습니다")).toBeTruthy();
  });

  it("surfaces a search failure instead of showing an empty answer", async () => {
    const search = vi.fn(async () => {
      throw new Error("history unavailable");
    });

    render(<AttentionAskPanel services={services({ search })} />);

    await act(async () => {
      fireEvent.change(screen.getByLabelText("Ask past conversations"), { target: { value: "무엇이든" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "질문" }));
    });

    expect(screen.getByRole("alert").textContent).toContain("history unavailable");
  });

  it("does not query for an empty question", async () => {
    const search = vi.fn(async (): Promise<AgentHistoryPage<AgentHistoryEntry>> => ({ items: [], nextCursor: null, partial: false, warnings: [] }));

    render(<AttentionAskPanel services={services({ search })} />);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "질문" }));
    });

    expect(search).not.toHaveBeenCalled();
  });
});
