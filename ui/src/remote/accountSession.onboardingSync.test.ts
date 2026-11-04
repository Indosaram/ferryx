import { afterEach, expect, it, vi } from "vitest";
import { syncAccountOnboarding } from "./accountSession";

afterEach(() => vi.unstubAllGlobals());

it("uses authenticated GET and PUT and validates the returned progress", async () => {
  const progress = { completedSteps: ["intro"], dismissed: false, updatedAt: 10 };
  const fetch = vi.fn().mockImplementation(async () => new Response(JSON.stringify(progress)));
  vi.stubGlobal("fetch", fetch);
  await expect(syncAccountOnboarding("https://relay.ferryx.dev", "token")).resolves.toEqual(progress);
  expect(fetch.mock.calls[0][0]).toBe("https://relay.ferryx.dev/api/v1/account/onboarding");
  expect(fetch.mock.calls[0][1]).toMatchObject({ method: "GET", headers: { Authorization: "Bearer token" } });
  await expect(syncAccountOnboarding("https://relay.ferryx.dev", "token", progress)).resolves.toEqual(progress);
  expect(fetch.mock.calls[1][1]).toMatchObject({ method: "PUT", body: JSON.stringify(progress) });
});

it("refuses machine progress in responses and propagates typed API failures", async () => {
  const fetch = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({
    completedSteps: ["enroll"], dismissed: false, updatedAt: 1,
  }))).mockResolvedValueOnce(new Response(JSON.stringify({ code: "UNAUTHORIZED", message: "expired" }), { status: 401 }));
  vi.stubGlobal("fetch", fetch);
  await expect(syncAccountOnboarding("https://relay.ferryx.dev", "token")).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
  await expect(syncAccountOnboarding("https://relay.ferryx.dev", "token")).rejects.toMatchObject({ code: "UNAUTHORIZED" });
});
