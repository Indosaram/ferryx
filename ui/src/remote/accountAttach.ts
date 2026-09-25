export interface AttachKeyPair {
  publicKey: string;
  privateKey: string;
}

export const ATTACH_KEY_UNSUPPORTED = "ATTACH_KEY_UNSUPPORTED";

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

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function bytesToBase64Url(bytes: Uint8Array): string {
  return bytesToBase64(bytes).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function base64UrlToBytes(base64url: string): Uint8Array {
  let base64 = base64url.replace(/-/g, "+").replace(/_/g, "/");
  while (base64.length % 4 !== 0) {
    base64 += "=";
  }
  return base64ToBytes(base64);
}

export async function validateAttachKeyPair(keyPair: AttachKeyPair): Promise<boolean> {
  if (
    typeof crypto === "undefined" ||
    !crypto.subtle ||
    typeof crypto.subtle.importKey !== "function"
  ) {
    throw new Error(`${ATTACH_KEY_UNSUPPORTED}: crypto.subtle.importKey is unavailable`);
  }
  try {
    const pubBytes = base64ToBytes(keyPair.publicKey);
    const privBytes = base64ToBytes(keyPair.privateKey);
    if (pubBytes.length !== 32 || privBytes.length !== 32) {
      return false;
    }
    const jwk = {
      kty: "OKP",
      crv: "X25519",
      d: bytesToBase64Url(privBytes),
      x: bytesToBase64Url(pubBytes),
    };
    await crypto.subtle.importKey("jwk", jwk, { name: "X25519" }, true, ["deriveBits"]);
    return true;
  } catch {
    return false;
  }
}

export async function getStoredAttachKey(): Promise<AttachKeyPair | null> {
  try {
    const db = await openDb();
    const stored = await new Promise<AttachKeyPair | null>((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, "readonly");
      const store = tx.objectStore(STORE_NAME);
      const req = store.get(ATTACH_KEY_ID);
      req.onsuccess = () => resolve((req.result as AttachKeyPair) ?? null);
      req.onerror = () => reject(req.error);
    });
    if (!stored) return null;
    const isValid = await validateAttachKeyPair(stored);
    if (!isValid) return null;
    return stored;
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

export async function getOrCreateAttachKey(): Promise<AttachKeyPair> {
  const existing = await getStoredAttachKey();
  if (existing) return existing;

  if (
    typeof crypto === "undefined" ||
    !crypto.subtle ||
    typeof crypto.subtle.generateKey !== "function" ||
    typeof crypto.subtle.exportKey !== "function" ||
    typeof crypto.subtle.importKey !== "function"
  ) {
    throw new Error(`${ATTACH_KEY_UNSUPPORTED}: SubtleCrypto X25519 operations are unavailable`);
  }

  let cryptoKeyPair: CryptoKeyPair;
  try {
    cryptoKeyPair = (await crypto.subtle.generateKey(
      { name: "X25519" },
      true,
      ["deriveBits"],
    )) as CryptoKeyPair;
  } catch (err) {
    throw new Error(`${ATTACH_KEY_UNSUPPORTED}: Failed to generate X25519 keypair: ${String(err)}`);
  }

  let rawPub: Uint8Array;
  let rawPriv: Uint8Array;
  try {
    rawPub = new Uint8Array(await crypto.subtle.exportKey("raw", cryptoKeyPair.publicKey));
    const privJwk = await crypto.subtle.exportKey("jwk", cryptoKeyPair.privateKey);
    if (!privJwk.d) {
      throw new Error("Exported X25519 private key JWK missing 'd' parameter");
    }
    rawPriv = base64UrlToBytes(privJwk.d);
  } catch (err) {
    throw new Error(`${ATTACH_KEY_UNSUPPORTED}: Failed to export X25519 keypair: ${String(err)}`);
  }

  if (rawPub.length !== 32 || rawPriv.length !== 32) {
    throw new Error(`${ATTACH_KEY_UNSUPPORTED}: Generated X25519 keys must be exactly 32 raw bytes`);
  }

  const jwk = {
    kty: "OKP",
    crv: "X25519",
    d: bytesToBase64Url(rawPriv),
    x: bytesToBase64Url(rawPub),
  };
  await crypto.subtle.importKey("jwk", jwk, { name: "X25519" }, true, ["deriveBits"]);

  const publicKey = bytesToBase64(rawPub);
  const privateKey = bytesToBase64(rawPriv);
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
  return `${protocol}//${base.host}/tunnel/opaque/${encodeURIComponent(sessionId)}`;
}
