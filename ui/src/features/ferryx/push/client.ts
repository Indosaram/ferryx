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

export class PushClient {
  constructor(readonly api: PushApi, readonly registration?: ServiceWorkerRegistration) {}

  async enable(_showBody = false): Promise<PushState> {
    if (typeof Notification !== "undefined" && Notification.permission === "denied") {
      return "denied";
    }
    try {
      let sub = await this.registration?.pushManager?.getSubscription?.();
      if (!sub) {
        sub = await (this.registration?.pushManager as unknown as { subscribe?: (opts?: unknown) => Promise<unknown> })?.subscribe?.({
          userVisibleOnly: true,
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
    const sub = await this.registration?.pushManager?.getSubscription?.();
    if (sub) {
      await this.api.request("/push/unsubscribe", { endpoint: (sub as { endpoint?: string }).endpoint });
      await (sub as { unsubscribe?: () => Promise<unknown> }).unsubscribe?.();
    }
    return "disabled";
  }
}
