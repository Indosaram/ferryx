/**
 * Registers the current page for Web Push notifications (used to alert the
 * user when an agent transitions into a waiting/blocked state) and forwards
 * the resulting subscription to the remote gateway's push endpoint.
 *
 * Web Push requires a Secure Context (HTTPS or localhost) plus a registered
 * service worker with push support. When either is unavailable this
 * function resolves to `false` instead of throwing, so callers can treat it
 * as a best-effort enhancement.
 */
export async function registerPushSubscription(
  apiBaseUrl: string,
  vapidPublicKey?: string,
): Promise<boolean> {
  if (typeof window === "undefined") {
    return false;
  }

  if (!window.isSecureContext) {
    return false;
  }

  if (!("serviceWorker" in navigator) || !("PushManager" in window)) {
    return false;
  }

  try {
    const registration = await navigator.serviceWorker.ready.catch(
      () => navigator.serviceWorker.register("/service-worker.js"),
    );

    if (!registration || !registration.pushManager) {
      return false;
    }

    let subscription = await registration.pushManager.getSubscription();
    if (!subscription) {
      const subscribeOptions: PushSubscriptionOptionsInit = {
        userVisibleOnly: true,
      };
      if (vapidPublicKey) {
        subscribeOptions.applicationServerKey = urlBase64ToUint8Array(vapidPublicKey) as BufferSource;
      }
      subscription = await registration.pushManager.subscribe(subscribeOptions);
    }

    const json = subscription.toJSON();
    if (!json.endpoint || !json.keys?.p256dh || !json.keys?.auth) {
      return false;
    }

    const base = apiBaseUrl.replace(/\/$/, "");
    const res = await fetch(`${base}/api/push/subscribe`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        endpoint: json.endpoint,
        keys: {
          p256dh: json.keys.p256dh,
          auth: json.keys.auth,
        },
      }),
    });

    return res.ok;
  } catch {
    return false;
  }
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
