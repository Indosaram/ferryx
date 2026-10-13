export type PushState = "loading" | "unsupported" | "insecure" | "denied" | "disabled" | "enabled" | "busy" | "error";

export function secureTaskLink(value: unknown, origin: string): string | null {
  if (typeof value !== "string") return null;
  if (!value.startsWith("/#task=")) return null;
  if (value.startsWith("//")) return null;
  const taskVal = value.slice("/#task=".length);
  if (!taskVal) return null;
  try {
    const url = new URL(value, origin);
    if (url.origin !== origin) return null;
    if (url.pathname !== "/") return null;
    if (url.search !== "") return null;
    if (!url.hash.startsWith("#task=") || url.hash === "#task=") return null;
    return url.href;
  } catch {
    return null;
  }
}

export interface PushApi { request(path: string, body?: unknown): Promise<unknown> }

function urlBase64ToUint8Array(value: string): Uint8Array {
  const base64 = (value + "=".repeat((4 - (value.length % 4)) % 4)).replace(/-/g, "+").replace(/_/g, "/");
  const raw = atob(base64);
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

export class PushClient {
  constructor(readonly api: PushApi, readonly registration?: ServiceWorkerRegistration, readonly vapidPublicKey?: string) {}

  async enable(_showBody = false): Promise<PushState> {
    if (typeof Notification !== "undefined" && Notification.permission === "denied") {
      return "denied";
    }
    try {
      let sub = await this.registration?.pushManager?.getSubscription?.();
      if (!sub) {
        // Browsers reject subscribe() without applicationServerKey; fail closed instead.
        if (!this.vapidPublicKey) return "error";
        sub = await (this.registration?.pushManager as unknown as { subscribe?: (opts?: unknown) => Promise<unknown> })?.subscribe?.({
          userVisibleOnly: true,
          applicationServerKey: urlBase64ToUint8Array(this.vapidPublicKey),
        }) as PushSubscription | undefined;
      }
      if (sub) {
        await this.api.request("/push/subscribe", sub);
      }
      return "enabled";
    } catch {
      return "error";
    }
  }

  async disable(): Promise<PushState> {
    try {
      const sub = await this.registration?.pushManager?.getSubscription?.();
      if (sub) {
        await this.api.request("/push/unsubscribe", { endpoint: (sub as { endpoint?: string }).endpoint });
        await (sub as { unsubscribe?: () => Promise<unknown> }).unsubscribe?.();
      }
      return "disabled";
    } catch {
      return "error";
    }
  }
}
