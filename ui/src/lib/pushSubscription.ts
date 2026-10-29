import { getRemoteAuthToken } from "./remoteClient";

/**
 * Path of the shipped service worker (`ui/public/sw.js`). The app bootstrap in
 * `main.tsx` and the fallback registration below must both use this constant:
 * a divergent literal 404s the fallback and push registration then degrades to
 * `false` with no diagnostic.
 */
export const SERVICE_WORKER_PATH = "/sw.js";

export type PhoneFinishedMode = "off" | "longTurns" | "everyTurn";

/** What the gateway pushes to this phone; sent with the subscription and on every change. */
export type PhoneAlertPreferences = {
  phoneNeedsInput: boolean;
  phoneFinishedMode: PhoneFinishedMode;
};

/** One gateway request (path relative to the gateway, bearer already attached by the caller). */
export type PushRequest = (path: string, init?: RequestInit) => Promise<Response>;

export type PhoneAlertState =
  | "unsupported"
  | "insecure"
  | "denied"
  | "dismissed"
  | "disabled"
  | "enabled"
  | "error";

function defaultRequest(apiBaseUrl: string): PushRequest {
  const base = apiBaseUrl.replace(/\/$/, "");
  return (path, init = {}) => {
    const token = getRemoteAuthToken();
    return fetch(`${base}${path}`, {
      ...init,
      headers: {
        ...(init.headers as Record<string, string> | undefined),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
    });
  };
}

function jsonPost(body: unknown): RequestInit {
  return { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) };
}

function pushCapable(): boolean {
  return typeof window !== "undefined" && "serviceWorker" in navigator && "PushManager" in window;
}

async function pushRegistration(): Promise<ServiceWorkerRegistration | null> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const registration = await Promise.race([
    navigator.serviceWorker.ready.catch(
      () => navigator.serviceWorker.register(SERVICE_WORKER_PATH),
    ),
    new Promise<null>((resolve) => {
      timeout = setTimeout(() => resolve(null), 3000);
    }),
  ]).finally(() => clearTimeout(timeout));
  return registration && registration.pushManager ? registration : null;
}

export type RegisterPushOptions = {
  vapidPublicKey?: string;
  preferences?: PhoneAlertPreferences;
  /** Defaults to plain fetch against `apiBaseUrl` with the stored device token. */
  request?: PushRequest;
};

/**
 * Registers the current page for Web Push notifications (used to alert the
 * user when an agent transitions into a waiting/blocked state) and forwards
 * the resulting subscription, with this phone's alert preferences, to the
 * remote gateway's push endpoint.
 *
 * Web Push requires a Secure Context (HTTPS or localhost) plus a registered
 * service worker with push support. When either is unavailable this
 * function resolves to `false` instead of throwing, so callers can treat it
 * as a best-effort enhancement. It never asks for notification permission.
 */
export async function registerPushSubscription(
  apiBaseUrl: string,
  options: RegisterPushOptions = {},
): Promise<boolean> {
  if (typeof window === "undefined" || !window.isSecureContext || !pushCapable()) {
    return false;
  }
  const request = options.request ?? defaultRequest(apiBaseUrl);

  try {
    const registration = await pushRegistration();
    if (!registration) return false;

    const applicationServerKey = options.vapidPublicKey ? urlBase64ToUint8Array(options.vapidPublicKey) : null;
    let subscription = await registration.pushManager.getSubscription();
    // A subscription bound to another gateway's key cannot carry this gateway's pushes.
    if (subscription && applicationServerKey && !sameKey(subscription.options?.applicationServerKey, applicationServerKey)) {
      await subscription.unsubscribe();
      subscription = null;
    }
    if (!subscription) {
      // Browsers reject subscribe() without applicationServerKey; fail closed instead.
      if (!applicationServerKey) return false;
      subscription = await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: applicationServerKey as BufferSource,
      });
    }

    const json = subscription.toJSON();
    if (!json.endpoint || !json.keys?.p256dh || !json.keys?.auth) {
      return false;
    }

    const res = await request("/api/v1/push/subscribe", jsonPost({
      endpoint: json.endpoint,
      keys: { p256dh: json.keys.p256dh, auth: json.keys.auth },
      ...(options.preferences ?? {}),
    }));
    return res.ok;
  } catch {
    return false;
  }
}

/** Where push stands on this browser, read without ever prompting for permission. */
export async function readPhoneAlertState(): Promise<PhoneAlertState> {
  if (typeof window === "undefined" || !pushCapable() || typeof Notification === "undefined") return "unsupported";
  if (!window.isSecureContext) return "insecure";
  if (Notification.permission === "denied") return "denied";
  if (Notification.permission !== "granted") return "disabled";
  try {
    const registration = await pushRegistration();
    const subscription = await registration?.pushManager.getSubscription();
    return subscription ? "enabled" : "disabled";
  } catch {
    return "disabled";
  }
}

/**
 * The "Phone alerts" tap: asks for notification permission (only here, once), then subscribes
 * with the gateway's VAPID key and registers the subscription with these preferences.
 */
export async function enablePhoneAlerts(
  request: PushRequest,
  preferences: PhoneAlertPreferences,
): Promise<PhoneAlertState> {
  if (typeof window === "undefined" || !pushCapable() || typeof Notification === "undefined") return "unsupported";
  if (!window.isSecureContext) return "insecure";
  let permission = Notification.permission;
  if (permission === "denied") return "denied";
  if (permission !== "granted") {
    permission = await Notification.requestPermission();
    if (permission === "denied") return "denied";
    if (permission !== "granted") return "dismissed";
  }
  try {
    const res = await request("/api/v1/push/vapid-public-key");
    if (!res.ok) return "error";
    const body = (await res.json()) as { publicKey?: unknown };
    if (typeof body.publicKey !== "string" || !body.publicKey) return "error";
    const ok = await registerPushSubscription("", { vapidPublicKey: body.publicKey, preferences, request });
    return ok ? "enabled" : "error";
  } catch {
    return "error";
  }
}

/** Removes the subscription on the gateway first; the browser copy goes only once that worked. */
export async function disablePhoneAlerts(request: PushRequest): Promise<PhoneAlertState> {
  if (!pushCapable()) return "unsupported";
  try {
    const registration = await pushRegistration();
    const subscription = await registration?.pushManager.getSubscription();
    if (subscription) {
      const res = await request("/api/v1/push/unsubscribe", jsonPost({ endpoint: subscription.endpoint }));
      if (!res.ok) return "error";
      await subscription.unsubscribe();
    }
    return "disabled";
  } catch {
    return "error";
  }
}

export async function updatePhoneAlertPreferences(
  request: PushRequest,
  preferences: PhoneAlertPreferences,
): Promise<boolean> {
  try {
    const res = await request("/api/v1/push/preferences", jsonPost(preferences));
    return res.ok;
  } catch {
    return false;
  }
}

export type PhoneTestResult = { ok: true; delivered: number } | { ok: false; message: string };

export async function sendPhoneTestAlert(request: PushRequest): Promise<PhoneTestResult> {
  try {
    const res = await request("/api/v1/push/test", { method: "POST" });
    if (!res.ok) {
      const reason = (await res.text().catch(() => "")).slice(0, 140);
      return { ok: false, message: reason || `Test alert failed (HTTP ${res.status}).` };
    }
    const body = (await res.json()) as { delivered?: unknown };
    return { ok: true, delivered: typeof body.delivered === "number" ? body.delivered : 0 };
  } catch (error) {
    return { ok: false, message: error instanceof Error ? error.message : "Test alert failed." };
  }
}

function sameKey(current: ArrayBuffer | null | undefined, wanted: Uint8Array): boolean {
  if (!current) return false;
  const bytes = new Uint8Array(current);
  return bytes.length === wanted.length && bytes.every((byte, index) => byte === wanted[index]);
}

function urlBase64ToUint8Array(base64String: string): Uint8Array {
  const padding = "=".repeat((4 - (base64String.length % 4)) % 4);
  const base64 = (base64String + padding).replace(/-/g, "+").replace(/_/g, "/");
  const rawData = atob(base64);
  const outputArray = new Uint8Array(rawData.length);
  for (let i = 0; i < rawData.length; i++) {
    outputArray[i] = rawData.charCodeAt(i);
  }
  return outputArray;
}
