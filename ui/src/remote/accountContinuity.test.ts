import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearStoredAccountSessionToken, getAccountContinuity, getAccountLastSelectedTarget,
  getStoredAccountId, putAccountContinuity, setAccountLastSelectedTarget, storeAccountSessionToken,
  getAccountPreferredSessionId, setAccountPreferredSessionId,
  getStoredAccountSessionToken,
  resolveStoredAccountId,
  AccountSessionError,
  ACCOUNT_SESSION_CHANGED_EVENT,
  makeAccountLastSelectedTargetKey, clearAccountLastSelectedTarget,
} from "./accountSession";

const origin = "https://relay.example.com";
const target = { machineId: "machine", workspaceId: "workspace", worktreeSlug: null, sessionId: "exact" };
const pointer = { ...target, revision: 7, updatedAt: 10 };

function storedValue(key: string): string | null {
  const raw = window.localStorage.getItem(key);
  if (raw === null) return null;
  try {
    const parsed = JSON.parse(raw);
    return parsed?.accountCacheIntent === 1 ? parsed.value : raw;
  } catch { return raw; }
}

function cacheReceiptKeys(): string[] {
  return Object.keys(window.localStorage).filter((key) => key.startsWith("ferryx.account.cache_migration."));
}

describe("account continuity protocol and identity", () => {
  beforeEach(() => {
    window.localStorage.clear();
    let intentId = 0;
    vi.spyOn(crypto, "randomUUID").mockImplementation(() => `00000000-0000-4000-8000-${String(++intentId).padStart(12, "0")}`);
  });
  afterEach(() => { vi.restoreAllMocks(); clearStoredAccountSessionToken(); vi.unstubAllGlobals(); });

  it("uses the authenticated camelCase CAS contract", async () => {
    const fetch = vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify(pointer))));
    vi.stubGlobal("fetch", fetch);
    expect(await getAccountContinuity(origin, "token")).toEqual(pointer);
    expect(await putAccountContinuity(origin, "token", 6, target)).toEqual(pointer);
    expect(fetch.mock.calls[1][0]).toBe(`${origin}/api/account/v1/continuity`);
    const request = fetch.mock.calls[1][1] as RequestInit;
    expect(request.headers).toMatchObject({ Authorization: "Bearer token" });
    expect(JSON.parse(request.body as string)).toEqual({ expectedRevision: 6, ...target });
  });

  it("allows local restore only when the continuity endpoint is missing", async () => {
    const transport = vi.fn();
    vi.stubGlobal("fetch", transport);
    transport.mockResolvedValueOnce(new Response(null, { status: 404 }));
    await expect(getAccountContinuity(origin, "token")).resolves.toBeNull();
    transport.mockResolvedValueOnce(new Response(null, { status: 404 }));
    await expect(putAccountContinuity(origin, "token", 0, target)).rejects.toMatchObject({ status: 404, code: "CONTINUITY_UNSUPPORTED" });
    for (const status of [401, 403, 503]) {
      transport.mockResolvedValueOnce(new Response(null, { status }));
      await expect(getAccountContinuity(origin, "token")).rejects.toMatchObject({ status });
      transport.mockResolvedValueOnce(new Response(null, { status }));
      await expect(putAccountContinuity(origin, "token", 0, target)).rejects.toMatchObject({ status, code: "CONTINUITY_WRITE_FAILED" });
    }
  });

  it("surfaces a CAS conflict without retrying", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      code: "CONTINUITY_CONFLICT", message: "conflict", details: { current: pointer },
    }), { status: 409 }));
    vi.stubGlobal("fetch", fetch);
    await expect(putAccountContinuity(origin, "token", 5, target)).rejects.toMatchObject({
      status: 409, code: "CONTINUITY_CONFLICT", details: { current: pointer },
    });
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it.each(["read", "write"] as const)("refuses cross-origin redirects on a continuity %s", async (operation) => {
    // A redirect would re-send the bearer to whatever host the answer names.
    const fetch = vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify(pointer))));
    vi.stubGlobal("fetch", fetch);
    if (operation === "read") await getAccountContinuity(origin, "token");
    else await putAccountContinuity(origin, "token", 6, target);
    expect(fetch.mock.calls[0][1]).toMatchObject({ redirect: "error" });
  });

  it.each(["read", "write"] as const)(
    "reports a non-JSON continuity %s body as a typed INVALID_RESPONSE",
    async (operation) => {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("<html>not json</html>", { status: 200 })));
      const request = operation === "read"
        ? getAccountContinuity(origin, "token")
        : putAccountContinuity(origin, "token", 6, target);
      // The raw SyntaxError from Response.json() must never escape to callers.
      const error = await request.then(() => null, (rejection: unknown) => rejection);
      expect(error).toBeInstanceOf(AccountSessionError);
      expect(error).not.toBeInstanceOf(SyntaxError);
      expect(error).toMatchObject({ code: "INVALID_RESPONSE", status: 200 });
    },
  );

  it.each(["read", "write"] as const)(
    "reports a JSON continuity %s body that is not a pointer as INVALID_RESPONSE",
    async (operation) => {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ revision: 0 }), { status: 200 })));
      const request = operation === "read"
        ? getAccountContinuity(origin, "token")
        : putAccountContinuity(origin, "token", 6, target);
      await expect(request).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
    },
  );

  it.each(["read", "write"] as const)("propagates cancellation to an in-flight %s", async (operation) => {
    let requestSignal: AbortSignal | undefined;
    const fetch = vi.fn().mockImplementation((_url: string, init: RequestInit) => {
      requestSignal = init.signal as AbortSignal;
      return new Promise<Response>((_resolve, reject) => {
        requestSignal!.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
      });
    });
    vi.stubGlobal("fetch", fetch);
    const controller = new AbortController();
    const request = operation === "read"
      ? getAccountContinuity(origin, "token", controller.signal)
      : putAccountContinuity(origin, "token", 6, target, controller.signal);
    const rejected = expect(request).rejects.toMatchObject({ name: "AbortError" });
    controller.abort();
    await rejected;
    expect(requestSignal?.aborted).toBe(true);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("isolates two accounts on one origin and retains exact identity", () => {
    storeAccountSessionToken("token-a", origin, "account-a");
    setAccountLastSelectedTarget(origin, target);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, "exact");
    storeAccountSessionToken("token-b", origin, "account-b");
    expect(getStoredAccountId(origin)).toBe("account-b");
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBeNull();
    storeAccountSessionToken("renewed-a", origin, "account-a");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
  });

  it("does not reuse identity or unscoped targets for legacy tokens", () => {
    storeAccountSessionToken("token-a", origin, "account-a");
    storeAccountSessionToken("legacy", origin);
    expect(getStoredAccountId(origin)).toBeNull();
    window.localStorage.setItem(`ferryx.account.last_target.${encodeURIComponent(origin)}`, JSON.stringify(target));
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    setAccountLastSelectedTarget(origin, target);
    storeAccountSessionToken("other-legacy", origin);
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
  });

  it("never copies the bearer token into the identity cache", () => {
    // The identity cache is read on every mount; a plaintext copy there is a second place the
    // token can leak from, so the record may only carry a one-way fingerprint of it.
    storeAccountSessionToken("token-secret", origin, "account-a");
    const stored = window.localStorage.getItem("ferryx.account.identity");
    expect(stored).not.toBeNull();
    expect(stored).not.toContain("token-secret");
    expect(Object.keys(JSON.parse(stored ?? "{}")).sort()).toEqual(["accountId", "origin", "tokenHash"]);
    expect(getStoredAccountId(origin)).toBe("account-a");
  });

  it("retires a plaintext identity record written before the token was hashed", () => {
    // An upgrade must not strand the login, and the old shape must not survive one read.
    storeAccountSessionToken("legacy-secret", origin);
    window.localStorage.setItem("ferryx.account.identity",
      JSON.stringify({ token: "legacy-secret", origin, accountId: "account-a" }));
    expect(getStoredAccountId(origin)).toBe("account-a");
    const migrated = window.localStorage.getItem("ferryx.account.identity");
    expect(migrated).not.toContain("legacy-secret");
    expect(JSON.parse(migrated ?? "{}").accountId).toBe("account-a");
    // A record for a different token still proves nothing.
    window.localStorage.setItem("ferryx.account.identity",
      JSON.stringify({ token: "another-secret", origin, accountId: "account-a" }));
    expect(getStoredAccountId(origin)).toBeNull();
  });

  it("uses bounded opaque deterministic legacy scopes without identity-domain collisions", () => {
    const credential = "legacy/secret?".repeat(1000);
    storeAccountSessionToken(credential, origin);
    const key = makeAccountLastSelectedTargetKey(origin);
    expect(key.split(".").at(-1)).toMatch(/^[a-f0-9]{64}$/);
    expect(key).not.toContain(credential);
    expect(key).not.toContain(encodeURIComponent(credential));
    setAccountLastSelectedTarget(origin, target);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, "exact");
    storeAccountSessionToken("other", origin);
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBeNull();
    storeAccountSessionToken(credential, origin);
    expect(makeAccountLastSelectedTargetKey(origin)).toBe(key);
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBe("exact");
    storeAccountSessionToken("identified", origin, `session:${credential}`);
    expect(makeAccountLastSelectedTargetKey(origin)).not.toBe(key);
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBeNull();
  });

  it("preserves unused pre-v2 caches across origins without adopting targets across accounts", () => {
    const oldKeys = [
      `ferryx.account.last_target.${encodeURIComponent(origin)}.session%3Aold-secret`,
      `ferryx.account.preferred_session.${encodeURIComponent(origin)}.session%3Aold-secret.machine.workspace.`,
      `ferryx.account.last_target.${encodeURIComponent("https://other.example.com")}.session%3Aother-secret`,
      `ferryx.account.last_target.${encodeURIComponent(origin)}`,
      `ferryx.account.preferred_session.${encodeURIComponent(origin)}.machine.workspace.`,
      `ferryx.account.last_target.${encodeURIComponent("https://other.example.com")}`,
      `ferryx.account.preferred_session.${encodeURIComponent("https://other.example.com")}.machine.workspace.`,
    ];
    for (const key of oldKeys) window.localStorage.setItem(key, JSON.stringify(target));
    for (const accountId of ["account-a", "account-b", undefined]) {
      storeAccountSessionToken(`new-secret-${accountId ?? "legacy"}`, origin, accountId);
      expect(getAccountLastSelectedTarget(origin)).toBeNull();
      expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBeNull();
      for (const key of oldKeys) expect(window.localStorage.getItem(key)).toBe(JSON.stringify(target));
    }
  });

  it("separates cache origins and retains only the current account on the current origin", () => {
    storeAccountSessionToken("token", origin, "account-a");
    const key = makeAccountLastSelectedTargetKey(origin);
    setAccountLastSelectedTarget(origin, target);
    const otherOrigin = "https://other.example.com";
    storeAccountSessionToken("token", otherOrigin, "account-a");
    expect(makeAccountLastSelectedTargetKey(otherOrigin)).not.toBe(key);
    expect(getAccountLastSelectedTarget(otherOrigin)).toBeNull();
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    storeAccountSessionToken("renewed", origin, "account-a");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
  });

  it("resolves an existing login through authenticated me and caches by canonical account", async () => {
    storeAccountSessionToken("legacy", origin);
    const fetch = vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ accountId: "account-a" }))));
    vi.stubGlobal("fetch", fetch);
    const changed = vi.fn();
    window.addEventListener(ACCOUNT_SESSION_CHANGED_EVENT, changed, { once: true });
    expect(await resolveStoredAccountId(origin, "legacy")).toBe("account-a");
    expect(changed).toHaveBeenCalledTimes(1);
    expect((changed.mock.calls[0][0] as CustomEvent).detail).toEqual({ origin, identityResolved: true });
    expect(fetch.mock.calls[0][0]).toBe(`${origin}/api/account/v1/me`);
    expect(fetch.mock.calls[0][1]).toMatchObject({ headers: { Authorization: "Bearer legacy" }, redirect: "error" });
    expect(getStoredAccountId(origin)).toBe("account-a");
    expect(await resolveStoredAccountId(origin, "legacy")).toBe("account-a");
    expect(fetch).toHaveBeenCalledTimes(1);
    setAccountLastSelectedTarget(origin, target);
    storeAccountSessionToken("renewed", origin, "account-a");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
  });

  it("migrates only existing exact-token canonical-origin v2 sources after authenticated me", async () => {
    const otherOrigin = "https://other.example.com";
    storeAccountSessionToken("other", origin);
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "other-token" });
    const otherTokenKey = makeAccountLastSelectedTargetKey(origin);
    storeAccountSessionToken("legacy", otherOrigin);
    setAccountLastSelectedTarget(otherOrigin, { ...target, sessionId: "other-origin" });
    const otherOriginKey = makeAccountLastSelectedTargetKey(otherOrigin);
    storeAccountSessionToken("legacy", origin);
    setAccountLastSelectedTarget(`${origin}/`, target);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, "exact");
    setAccountPreferredSessionId(origin, "machine", "workspace", "branch", "branch-session");
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    const preferredSources = Object.keys(window.localStorage).filter((key) => key.startsWith("ferryx.account.preferred_session.v2."));
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ accountId: "account-a" }))));
    expect(await resolveStoredAccountId(`${origin}/`, "legacy")).toBe("account-a");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBe("exact");
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", "branch")).toBe("branch-session");
    for (const key of [sourceKey, ...preferredSources]) expect(window.localStorage.getItem(key)).toBeNull();
    expect(window.localStorage.getItem(otherTokenKey)).not.toBeNull();
    expect(window.localStorage.getItem(otherOriginKey)).not.toBeNull();
    storeAccountSessionToken("renewed", origin, "account-a");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBe("exact");
  });

  it.each(["existing", "empty"] as const)("keeps the %s destination over unversioned legacy v2 sources", async (destination) => {
    storeAccountSessionToken("identified", origin, "account-a");
    const destinationKey = makeAccountLastSelectedTargetKey(origin);
    const destinationValue = destination === "empty" ? "" : JSON.stringify({ ...target, sessionId: "newer" });
    window.localStorage.setItem(destinationKey, destinationValue);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, destination === "empty" ? "" : "newer");
    storeAccountSessionToken("legacy", origin);
    setAccountLastSelectedTarget(origin, target);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, "exact");
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    window.localStorage.setItem(sourceKey, JSON.stringify(target));
    for (const key of Object.keys(window.localStorage).filter((key) => key.startsWith("ferryx.account.preferred_session.v2."))) {
      window.localStorage.setItem(key, storedValue(key)!);
    }
    vi.stubGlobal("fetch", vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ accountId: "account-a" })))));
    const write = vi.spyOn(Storage.prototype, "setItem");
    await resolveStoredAccountId(origin, "legacy");
    expect(window.localStorage.getItem(destinationKey)).toBe(destinationValue);
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBe(destination === "empty" ? null : "newer");
    expect(write.mock.calls.filter(([key]) => key.includes(".v2."))).toEqual([]);
    expect(window.localStorage.getItem(sourceKey)).toBeNull();
    clearAccountLastSelectedTarget(origin);
    window.localStorage.removeItem("ferryx.account.identity");
    await resolveStoredAccountId(origin, "legacy");
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
  });

  it("does not manufacture v2 state or adopt unscoped data during identity resolution", async () => {
    storeAccountSessionToken("legacy", origin);
    const unscoped = `ferryx.account.last_target.${encodeURIComponent(origin)}`;
    const preferred = `ferryx.account.preferred_session.${encodeURIComponent(origin)}.machine.workspace.`;
    window.localStorage.setItem(unscoped, JSON.stringify(target));
    window.localStorage.setItem(preferred, "previous-user-session");
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ accountId: "account-a" }))));
    const write = vi.spyOn(Storage.prototype, "setItem");
    await resolveStoredAccountId(origin, "legacy");
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBeNull();
    expect(write.mock.calls.filter(([key]) => key.includes(".v2."))).toEqual([]);
    expect(window.localStorage.getItem(unscoped)).toBe(JSON.stringify(target));
    expect(window.localStorage.getItem(preferred)).toBe("previous-user-session");
  });

  it("persists destinations before removing sources", async () => {
    storeAccountSessionToken("identified", origin, "account-a");
    const destinationKey = makeAccountLastSelectedTargetKey(origin);
    storeAccountSessionToken("legacy", origin);
    setAccountLastSelectedTarget(origin, target);
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    const removeItem = Storage.prototype.removeItem;
    const persisted: (string | null)[] = [];
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(function (this: Storage, key) {
      if (key === sourceKey) persisted.push(storedValue(destinationKey));
      removeItem.call(this, key);
    });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ accountId: "account-a" }))));
    await resolveStoredAccountId(origin, "legacy");
    expect(persisted).toEqual([JSON.stringify(target)]);
  });

  it("preserves sources and token-scoped access if identity persistence fails", async () => {
    storeAccountSessionToken("legacy", origin);
    setAccountLastSelectedTarget(origin, target);
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    const setItem = Storage.prototype.setItem;
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key, value) {
      if (key === "ferryx.account.identity") throw new DOMException("Full", "QuotaExceededError");
      setItem.call(this, key, value);
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ accountId: "account-a" }))));
    expect(await resolveStoredAccountId(origin, "legacy")).toBeNull();
    expect(getStoredAccountId(origin)).toBeNull();
    expect(storedValue(sourceKey)).toBe(JSON.stringify(target));
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
  });

  it("preserves each source when destination persistence fails", async () => {
    storeAccountSessionToken("legacy", origin);
    setAccountLastSelectedTarget(origin, target);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, "exact");
    const sources = Object.keys(window.localStorage).filter((key) => key.includes(".v2."));
    const setItem = Storage.prototype.setItem;
    const write = vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key, value) {
      if (key.includes(".v2.")) throw new DOMException("Full", "QuotaExceededError");
      setItem.call(this, key, value);
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ accountId: "account-a" })));
    vi.stubGlobal("fetch", fetch);
    expect(await resolveStoredAccountId(origin, "legacy")).toBe("account-a");
    for (const key of sources) expect(window.localStorage.getItem(key)).not.toBeNull();
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBeNull();
    write.mockRestore();
    expect(await resolveStoredAccountId(origin, "legacy")).toBe("account-a");
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBe("exact");
    for (const key of sources) expect(window.localStorage.getItem(key)).toBeNull();
  });

  it.each(["target", "clear"] as const)("applies newer T2 %s intent over T1 account state after me", async (intent) => {
    storeAccountSessionToken("T1", origin, "account-a");
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "old" });
    setAccountPreferredSessionId(origin, "machine", "workspace", null, "old");
    storeAccountSessionToken("T2", origin);
    let complete!: (response: Response) => void;
    const fetch = vi.fn().mockImplementation(() => new Promise<Response>((resolve) => { complete = resolve; }));
    vi.stubGlobal("fetch", fetch);
    const pending = resolveStoredAccountId(origin, "T2");
    if (intent === "clear") clearAccountLastSelectedTarget(origin);
    else setAccountLastSelectedTarget(origin, target);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, intent === "clear" ? "" : "exact");
    complete(new Response(JSON.stringify({ accountId: "account-a" })));
    expect(await pending).toBe("account-a");
    expect(getAccountLastSelectedTarget(origin)).toEqual(intent === "clear" ? null : expect.objectContaining(target));
    expect(getAccountPreferredSessionId(origin, "machine", "workspace", null)).toBe(intent === "clear" ? null : "exact");
    await resolveStoredAccountId(origin, "T2");
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it.each(["receipt", "consumed"] as const)("preserves sources when %s persistence fails and retries without replaying later account intent", async (stage) => {
    storeAccountSessionToken("T2", origin);
    setAccountLastSelectedTarget(origin, target);
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    const sourceValue = window.localStorage.getItem(sourceKey);
    const setItem = Storage.prototype.setItem;
    const write = vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key, value) {
      if (key.startsWith("ferryx.account.cache_migration.") && (stage === "receipt" || JSON.parse(value).consumed)) {
        throw new DOMException("Full", "QuotaExceededError");
      }
      setItem.call(this, key, value);
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ accountId: "account-a" })));
    vi.stubGlobal("fetch", fetch);
    await resolveStoredAccountId(origin, "T2");
    expect(window.localStorage.getItem(sourceKey)).toBe(sourceValue);
    if (stage === "receipt") expect(getAccountLastSelectedTarget(origin)).toBeNull();
    else expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
    write.mockRestore();
    clearAccountLastSelectedTarget(origin);
    await resolveStoredAccountId(origin, "T2");
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    expect(window.localStorage.getItem(sourceKey)).toBeNull();
  });

  it("distinguishes a consumed leftover from a genuine same-value token-scoped write", async () => {
    storeAccountSessionToken("T2", origin);
    setAccountLastSelectedTarget(origin, target);
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    const original = window.localStorage.getItem(sourceKey);
    const removeItem = Storage.prototype.removeItem;
    const remove = vi.spyOn(Storage.prototype, "removeItem").mockImplementation(function (this: Storage, key) {
      if (key === sourceKey) throw new Error("blocked removal");
      removeItem.call(this, key);
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.stubGlobal("fetch", vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ accountId: "account-a" })))));
    await resolveStoredAccountId(origin, "T2");
    clearAccountLastSelectedTarget(origin);
    window.localStorage.removeItem("ferryx.account.identity");
    await resolveStoredAccountId(origin, "T2");
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    window.localStorage.removeItem("ferryx.account.identity");
    setAccountLastSelectedTarget(origin, target);
    expect(window.localStorage.getItem(sourceKey)).not.toBe(original);
    remove.mockRestore();
    await resolveStoredAccountId(origin, "T2");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
    expect(window.localStorage.getItem(sourceKey)).toBeNull();
  });

  it("does not throw or suppress session notification when optional identity persistence fails after token storage", async () => {
    storeAccountSessionToken("T1", origin, "account-a");
    const setItem = Storage.prototype.setItem;
    const write = vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key, value) {
      if (key === "ferryx.account.identity") throw new DOMException("Full", "QuotaExceededError");
      setItem.call(this, key, value);
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const changed = vi.fn();
    window.addEventListener(ACCOUNT_SESSION_CHANGED_EVENT, changed);
    try {
      expect(() => storeAccountSessionToken("T2", origin, "account-b")).not.toThrow();
      expect(changed).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener(ACCOUNT_SESSION_CHANGED_EVENT, changed);
    }
    expect(getStoredAccountSessionToken(origin)).toBe("T2");
    expect(getStoredAccountId(origin)).toBeNull();
    write.mockRestore();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ accountId: "account-b" }))));
    expect(await resolveStoredAccountId(origin, "T2")).toBe("account-b");
  });

  it("never reassigns a consumed source generation to another account", async () => {
    storeAccountSessionToken("T2", origin);
    setAccountLastSelectedTarget(origin, target);
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    const removeItem = Storage.prototype.removeItem;
    const remove = vi.spyOn(Storage.prototype, "removeItem").mockImplementation(function (this: Storage, key) {
      if (key === sourceKey) throw new Error("blocked removal");
      removeItem.call(this, key);
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const fetch = vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ accountId: "account-a" }))));
    vi.stubGlobal("fetch", fetch);
    await resolveStoredAccountId(origin, "T2");
    remove.mockRestore();
    window.localStorage.removeItem("ferryx.account.identity");
    fetch.mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ accountId: "account-b" }))));
    await resolveStoredAccountId(origin, "T2");
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    expect(window.localStorage.getItem(sourceKey)).not.toBeNull();
    storeAccountSessionToken("renewed-a", origin, "account-a");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
  });

  it.each([false, true])("does not resurrect a source after removal failure (clear initially blocked: %s)", async (blockedClear) => {
    storeAccountSessionToken("legacy", origin);
    setAccountLastSelectedTarget(origin, target);
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    const removeItem = Storage.prototype.removeItem;
    const remove = vi.spyOn(Storage.prototype, "removeItem").mockImplementation(function (this: Storage, key) {
      if (key === sourceKey) throw new Error("blocked removal");
      removeItem.call(this, key);
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.stubGlobal("fetch", vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ accountId: "account-a" })))));
    await resolveStoredAccountId(origin, "legacy");
    expect(window.localStorage.getItem(sourceKey)).not.toBeNull();
    if (blockedClear) {
      const destinationKey = makeAccountLastSelectedTargetKey(origin);
      const setItem = Storage.prototype.setItem;
      const write = vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key, value) {
        if (key === destinationKey) throw new DOMException("Full", "QuotaExceededError");
        setItem.call(this, key, value);
      });
      clearAccountLastSelectedTarget(origin);
      expect(storedValue(sourceKey)).toBe(JSON.stringify(target));
      expect(getAccountLastSelectedTarget(origin)).toMatchObject(target);
      write.mockRestore();
    }
    clearAccountLastSelectedTarget(origin);
    remove.mockRestore();
    window.localStorage.removeItem("ferryx.account.identity");
    await resolveStoredAccountId(origin, "legacy");
    expect(getAccountLastSelectedTarget(origin)).toBeNull();
    expect(window.localStorage.getItem(sourceKey)).toBeNull();
  });

  it("skips unchanged cache writes", () => {
    storeAccountSessionToken("token", origin, "account-a");
    setAccountLastSelectedTarget(origin, target);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, "exact");
    const write = vi.spyOn(Storage.prototype, "setItem");
    setAccountLastSelectedTarget(origin, target);
    setAccountPreferredSessionId(origin, "machine", "workspace", null, "exact");
    expect(write).not.toHaveBeenCalled();
    clearAccountLastSelectedTarget(origin);
    write.mockClear();
    clearAccountLastSelectedTarget(origin);
    expect(write).not.toHaveBeenCalled();
  });

  it("rebinds a migration receipt that names an older source generation", async () => {
    storeAccountSessionToken("T1", origin);
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "legacy" });
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    storeAccountSessionToken("T1", origin, "account-a");
    const destinationKey = makeAccountLastSelectedTargetKey(origin);
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "account" });
    const [receiptKey] = cacheReceiptKeys();
    expect(JSON.parse(window.localStorage.getItem(receiptKey)!).source).toBe(window.localStorage.getItem(sourceKey));

    window.localStorage.removeItem("ferryx.account.identity");
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "relapsed" });
    const generation = window.localStorage.getItem(sourceKey);
    expect(JSON.parse(window.localStorage.getItem(receiptKey)!).source).not.toBe(generation);
    storeAccountSessionToken("T1", origin, "account-a");
    const accountState = window.localStorage.getItem(destinationKey);
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "newest" });

    expect(JSON.parse(window.localStorage.getItem(receiptKey)!)).toEqual({
      source: generation, destination: destinationKey, previous: accountState, consumed: false,
    });
    expect(accountState).not.toBe(window.localStorage.getItem(destinationKey));

    expect(await resolveStoredAccountId(origin, "T1")).toBe("account-a");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject({ sessionId: "newest" });
    expect(window.localStorage.getItem(sourceKey)).toBeNull();
    expect(JSON.parse(window.localStorage.getItem(receiptKey)!).consumed).toBe(true);
  });

  it("preserves an unchanged account reselect when its receipt was lost", async () => {
    storeAccountSessionToken("T1", origin);
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "stale" });
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    storeAccountSessionToken("T1", origin, "account-a");
    const destinationKey = makeAccountLastSelectedTargetKey(origin);
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "newest" });
    for (const key of cacheReceiptKeys()) window.localStorage.removeItem(key);

    const reselected = window.localStorage.getItem(destinationKey);
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "newest" });

    expect(JSON.parse(window.localStorage.getItem(cacheReceiptKeys()[0])!)).toEqual({
      source: window.localStorage.getItem(sourceKey),
      destination: destinationKey,
      previous: reselected,
      consumed: false,
    });
    expect(window.localStorage.getItem(destinationKey)).not.toBe(reselected);

    expect(await resolveStoredAccountId(origin, "T1")).toBe("account-a");
    expect(getAccountLastSelectedTarget(origin)).toMatchObject({ sessionId: "newest" });
    expect(window.localStorage.getItem(sourceKey)).toBeNull();
  });

  it.each(["previous", "garbage"] as const)("keeps both selections when a %s receipt is malformed", async (shape) => {
    storeAccountSessionToken("T1", origin);
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "pending" });
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    storeAccountSessionToken("T1", origin, "account-a");
    setAccountLastSelectedTarget(origin, { ...target, sessionId: "account" });
    const destinationKey = makeAccountLastSelectedTargetKey(origin);
    const source = window.localStorage.getItem(sourceKey)!;
    window.localStorage.setItem(cacheReceiptKeys()[0], shape === "previous"
      ? JSON.stringify({ source, destination: destinationKey, previous: 7, consumed: true })
      : "not json");
    vi.spyOn(console, "warn").mockImplementation(() => {});

    await resolveStoredAccountId(origin, "T1");

    expect(getAccountLastSelectedTarget(origin)).toMatchObject({ sessionId: "account" });
    expect(storedValue(sourceKey)).toBe(JSON.stringify({ ...target, sessionId: "pending" }));
  });

  it.each(["token", "origin", "logout", "abort", "account"] as const)("discards identity arriving after %s changes", async (change) => {
    storeAccountSessionToken("legacy", origin);
    setAccountLastSelectedTarget(origin, target);
    const sourceKey = makeAccountLastSelectedTargetKey(origin);
    let complete!: (response: Response) => void;
    vi.stubGlobal("fetch", vi.fn().mockImplementation(() => new Promise<Response>((resolve) => { complete = resolve; })));
    const controller = new AbortController();
    const pending = resolveStoredAccountId(origin, "legacy", controller.signal);
    if (change === "token") storeAccountSessionToken("other", origin, "account-b");
    if (change === "origin") storeAccountSessionToken("legacy", "https://other.example.com", "account-b");
    if (change === "logout") clearStoredAccountSessionToken();
    if (change === "abort") controller.abort();
    if (change === "account") storeAccountSessionToken("legacy", origin, "account-b");
    const changed = vi.fn();
    window.addEventListener(ACCOUNT_SESSION_CHANGED_EVENT, changed);
    complete(new Response(JSON.stringify({ accountId: "account-a" })));
    expect(await pending).toBeNull();
    window.removeEventListener(ACCOUNT_SESSION_CHANGED_EVENT, changed);
    expect(changed).not.toHaveBeenCalled();
    expect(getStoredAccountId(origin)).not.toBe("account-a");
    if (change !== "logout") expect(storedValue(sourceKey)).toBe(JSON.stringify(target));
    if (change === "origin") expect(getStoredAccountId("https://other.example.com")).toBe("account-b");
  });

  it("rejects malformed identity without caching it", async () => {
    storeAccountSessionToken("legacy", origin);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ accountId: 123 }))));
    await expect(resolveStoredAccountId(origin, "legacy")).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
    expect(getStoredAccountId(origin)).toBeNull();
  });

  it("rechecks the credential after the identity response body resolves", async () => {
    storeAccountSessionToken("legacy", origin);
    let completeBody!: (value: unknown) => void;
    let bodyStarted!: () => void;
    const readingBody = new Promise<void>((resolve) => { bodyStarted = resolve; });
    const response = new Response();
    vi.spyOn(response, "json").mockImplementation(() => {
      bodyStarted();
      return new Promise((resolve) => { completeBody = resolve; });
    });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(response));
    const pending = resolveStoredAccountId(origin, "legacy");
    await readingBody;
    storeAccountSessionToken("other", origin, "account-b");
    const changed = vi.fn();
    window.addEventListener(ACCOUNT_SESSION_CHANGED_EVENT, changed);
    completeBody({ accountId: "account-a" });
    expect(await pending).toBeNull();
    window.removeEventListener(ACCOUNT_SESSION_CHANGED_EVENT, changed);
    expect(changed).not.toHaveBeenCalled();
    expect(getStoredAccountId(origin)).toBe("account-b");
  });
});
