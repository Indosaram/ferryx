export interface AttachKeyPair {
  publicKey: string;
  privateKey: string;
}

const DB_NAME = "ferryx.account";
const DB_VERSION = 1;
const STORE_NAME = "keys";
const ATTACH_KEY_ID = "ferryx.account.attachKey";

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === "undefined") {
      reject(new Error("IndexedDB is not available in this environment"));
      return;
    }
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export async function getStoredAttachKey(): Promise<AttachKeyPair | null> {
  try {
    const db = await openDb();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, "readonly");
      const store = tx.objectStore(STORE_NAME);
      const req = store.get(ATTACH_KEY_ID);
      req.onsuccess = () => resolve((req.result as AttachKeyPair) ?? null);
      req.onerror = () => reject(req.error);
    });
  } catch {
    return null;
  }
}

export async function storeAttachKey(keyPair: AttachKeyPair): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const req = store.put(keyPair, ATTACH_KEY_ID);
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error);
  });
}

function generateRandomBase64(bytesCount: number): string {
  if (typeof crypto === "undefined" || typeof crypto.getRandomValues !== "function") {
    throw new Error("SECURE_CRYPTO_UNAVAILABLE: crypto.getRandomValues is required");
  }
  const bytes = new Uint8Array(bytesCount);
  crypto.getRandomValues(bytes);
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

export async function getOrCreateAttachKey(): Promise<AttachKeyPair> {
  const existing = await getStoredAttachKey();
  if (existing) return existing;

  const privateKey = generateRandomBase64(32);
  const publicKey = generateRandomBase64(32);
  const keyPair: AttachKeyPair = { publicKey, privateKey };
  try {
    await storeAttachKey(keyPair);
  } catch {
  }
  return keyPair;
}

export function buildAttachSocketUrl(
  baseUrl: string,
  sessionId: string,
  attachKey: AttachKeyPair | null,
): string {
  if (!attachKey || !attachKey.privateKey || !attachKey.publicKey) {
    throw new Error(
      "MISSING_ATTACH_KEY: Account attach channel requires an attach key. Falling back to legacy relay ticket route is forbidden.",
    );
  }
  const base = new URL(baseUrl);
  const protocol = base.protocol === "https:" ? "wss:" : "ws:";
  const socketUrl = new URL(`${protocol}//${base.host}/api/v1/attach`);
  socketUrl.searchParams.set("sessionId", sessionId);
  socketUrl.searchParams.set("attachKey", attachKey.publicKey);
  return socketUrl.toString();
}
