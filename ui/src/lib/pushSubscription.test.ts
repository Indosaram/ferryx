import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  disablePhoneAlerts,
  enablePhoneAlerts,
  readPhoneAlertState,
  registerPushSubscription,
  SERVICE_WORKER_PATH,
  updatePhoneAlertPreferences,
} from "./pushSubscription";

const SHIPPED_SERVICE_WORKER = "/sw.js";
/** jsdom replaces the global URL class, so fs checks need a plain string path. */
const PUBLIC_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../../public");

const restorers: Array<() => void> = [];

function stubProperty(target: object, key: string, value: unknown): void {
  const original = Object.getOwnPropertyDescriptor(target, key);
  Object.defineProperty(target, key, { value, configurable: true, writable: true });
  restorers.push(() => {
    if (original) {
      Object.defineProperty(target, key, original);
      return;
    }
    Reflect.deleteProperty(target, key);
  });
}

/** jsdom exposes neither a Secure Context flag nor Service Worker globals. */
function installPushCapableBrowser(register: (path: string) => Promise<unknown>, ready: Promise<never>): void {
  stubProperty(window, "isSecureContext", true);
  stubProperty(window, "PushManager", class PushManager {});
  stubProperty(navigator, "serviceWorker", { ready, register });
}

afterEach(() => {
  while (restorers.length > 0) {
    restorers.pop()?.();
  }
  vi.unstubAllGlobals();
});

describe("pushSubscription service worker registration", () => {
  it("names the service worker the app ships", () => {
    expect(SERVICE_WORKER_PATH).toBe(SHIPPED_SERVICE_WORKER);
    expect(existsSync(join(PUBLIC_DIR, SERVICE_WORKER_PATH.replace(/^\//, "")))).toBe(true);
  });

  it("registers the shipped service worker when serviceWorker.ready rejects", async () => {
    const registration = {
      pushManager: {
        getSubscription: async () => ({
          toJSON: () => ({
            endpoint: "https://push.example.test/subscription",
            keys: { p256dh: "p256dh-key", auth: "auth-key" },
          }),
        }),
      },
    };
    const register = vi.fn(async () => registration);
    installPushCapableBrowser(register, Promise.reject(new Error("no active service worker")));
    stubProperty(globalThis, "fetch", vi.fn(async () => ({ ok: true })));

    await expect(registerPushSubscription("https://gateway.example.test")).resolves.toBe(true);

    expect(register).toHaveBeenCalledTimes(1);
    expect(register).toHaveBeenCalledWith(SHIPPED_SERVICE_WORKER);
  });
});

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

function installNotification(permission: NotificationPermission, answer: NotificationPermission = "granted") {
  const requestPermission = vi.fn(async () => {
    notification.permission = answer;
    return answer;
  });
  const notification = { permission, requestPermission };
  stubProperty(globalThis, "Notification", notification);
  return requestPermission;
}

function installPushManager(existing: unknown = null) {
  const created = {
    endpoint: "https://fcm.googleapis.com/fcm/send/abc",
    options: { applicationServerKey: new Uint8Array([1, 2, 3]).buffer },
    toJSON: () => ({ endpoint: "https://fcm.googleapis.com/fcm/send/abc", keys: { p256dh: "p256dh-key", auth: "auth-key" } }),
    unsubscribe: vi.fn(async () => true),
  };
  const pushManager = {
    getSubscription: vi.fn(async () => existing),
    subscribe: vi.fn(async () => created),
  };
  installPushCapableBrowser(vi.fn(), Promise.resolve({ pushManager } as never));
  return { pushManager, created };
}

const PREFS = { phoneNeedsInput: true, phoneFinishedMode: "longTurns" as const };

describe("phone alerts", () => {
  it("asks for permission once on enable, then subscribes with the gateway VAPID key and preferences", async () => {
    const requestPermission = installNotification("default", "granted");
    const { pushManager } = installPushManager();
    const request = vi.fn(async (path: string) =>
      path === "/api/push/vapid-public-key" ? jsonResponse({ publicKey: "AQID" }) : new Response(null, { status: 204 }));

    await expect(enablePhoneAlerts(request, PREFS)).resolves.toBe("enabled");

    expect(requestPermission).toHaveBeenCalledTimes(1);
    expect(pushManager.subscribe).toHaveBeenCalledTimes(1);
    expect(pushManager.subscribe).toHaveBeenCalledWith({
      userVisibleOnly: true,
      applicationServerKey: new Uint8Array([1, 2, 3]),
    });
    expect(requestPermission.mock.invocationCallOrder[0]).toBeLessThan(pushManager.subscribe.mock.invocationCallOrder[0]);
    const subscribeCall = request.mock.calls.find(([path]) => path === "/api/push/subscribe") as unknown as [string, RequestInit];
    expect(JSON.parse(String(subscribeCall[1].body))).toEqual({
      endpoint: "https://fcm.googleapis.com/fcm/send/abc",
      keys: { p256dh: "p256dh-key", auth: "auth-key" },
      phoneNeedsInput: true,
      phoneFinishedMode: "longTurns",
    });
  });

  it("never subscribes when this browser blocks notifications", async () => {
    const requestPermission = installNotification("denied");
    const { pushManager } = installPushManager();
    const request = vi.fn();

    await expect(enablePhoneAlerts(request, PREFS)).resolves.toBe("denied");

    expect(requestPermission).not.toHaveBeenCalled();
    expect(pushManager.subscribe).not.toHaveBeenCalled();
    expect(request).not.toHaveBeenCalled();
  });

  it("stops when the permission prompt is denied", async () => {
    installNotification("default", "denied");
    const { pushManager } = installPushManager();

    await expect(enablePhoneAlerts(vi.fn(), PREFS)).resolves.toBe("denied");
    expect(pushManager.subscribe).not.toHaveBeenCalled();
  });

  it("reads the current state without asking for permission", async () => {
    const requestPermission = installNotification("default");
    installPushManager();

    await expect(readPhoneAlertState()).resolves.toBe("disabled");
    expect(requestPermission).not.toHaveBeenCalled();
  });

  it("replaces a subscription bound to another gateway key", async () => {
    installNotification("granted");
    const stale = {
      options: { applicationServerKey: new Uint8Array([9, 9]).buffer },
      unsubscribe: vi.fn(async () => true),
    };
    const { pushManager } = installPushManager(stale);
    const request = vi.fn(async (path: string) =>
      path === "/api/push/vapid-public-key" ? jsonResponse({ publicKey: "AQID" }) : new Response(null, { status: 204 }));

    await expect(enablePhoneAlerts(request, PREFS)).resolves.toBe("enabled");
    expect(stale.unsubscribe).toHaveBeenCalledTimes(1);
    expect(pushManager.subscribe).toHaveBeenCalledTimes(1);
  });

  it("posts preference changes and keeps the browser subscription when the gateway refuses removal", async () => {
    const request = vi.fn(async () => new Response(null, { status: 204 }));
    await expect(updatePhoneAlertPreferences(request, { phoneNeedsInput: false, phoneFinishedMode: "everyTurn" })).resolves.toBe(true);
    expect(request).toHaveBeenCalledWith("/api/push/preferences", expect.objectContaining({
      method: "POST",
      body: JSON.stringify({ phoneNeedsInput: false, phoneFinishedMode: "everyTurn" }),
    }));

    const existing = { endpoint: "https://fcm.googleapis.com/x", unsubscribe: vi.fn(async () => true) };
    installPushManager(existing);
    await expect(disablePhoneAlerts(vi.fn(async () => new Response(null, { status: 500 })))).resolves.toBe("error");
    expect(existing.unsubscribe).not.toHaveBeenCalled();
  });
});
