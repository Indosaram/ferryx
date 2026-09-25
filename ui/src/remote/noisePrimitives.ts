import { chacha20poly1305 } from "@noble/ciphers/chacha.js";
import { blake2s } from "@noble/hashes/blake2.js";
import { hmac } from "@noble/hashes/hmac.js";
import type { NoisePrimitives } from "./attachFraming";

const PKCS8_X25519_PREFIX = new Uint8Array([
  0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e,
  0x04, 0x22, 0x04, 0x20,
]);

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

function bytesToBase64Url(bytes: Uint8Array): string {
  return bytesToBase64(bytes)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function base64UrlToBytes(base64url: string): Uint8Array {
  let base64 = base64url.replace(/-/g, "+").replace(/_/g, "/");
  while (base64.length % 4 !== 0) {
    base64 += "=";
  }
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function concatBytes(...arrays: Uint8Array[]): Uint8Array {
  const totalLength = arrays.reduce((sum, a) => sum + a.length, 0);
  const result = new Uint8Array(totalLength);
  let offset = 0;
  for (const arr of arrays) {
    result.set(arr, offset);
    offset += arr.length;
  }
  return result;
}

function formatNonce(nonce: number | Uint8Array): Uint8Array {
  if (typeof nonce === "number") {
    if (nonce < 0 || !Number.isSafeInteger(nonce)) {
      throw new Error(`AEAD_NONCE_INVALID: invalid nonce number ${nonce}`);
    }
    const buf = new Uint8Array(12);
    const view = new DataView(buf.buffer, buf.byteOffset, 12);
    view.setBigUint64(4, BigInt(nonce), true);
    return buf;
  }
  if (nonce instanceof Uint8Array) {
    if (nonce.length !== 12) {
      throw new Error(`AEAD_NONCE_INVALID: expected 12 bytes nonce, got ${nonce.length}`);
    }
    return nonce;
  }
  throw new Error("AEAD_NONCE_INVALID: expected number or 12-byte Uint8Array");
}

export function createNoisePrimitives(): NoisePrimitives {
  return {
    async generateEphemeralKey(): Promise<{ publicKey: Uint8Array; privateKey: Uint8Array }> {
      if (
        typeof crypto === "undefined" ||
        !crypto.subtle ||
        typeof crypto.subtle.generateKey !== "function" ||
        typeof crypto.subtle.exportKey !== "function"
      ) {
        throw new Error("WEBCRYPTO_UNAVAILABLE: SubtleCrypto X25519 operations are unavailable");
      }

      const keyPair = (await crypto.subtle.generateKey(
        { name: "X25519" },
        true,
        ["deriveBits"],
      )) as CryptoKeyPair;

      const publicKey = new Uint8Array(await crypto.subtle.exportKey("raw", keyPair.publicKey));

      try {
        const pkcs8 = new Uint8Array(await crypto.subtle.exportKey("pkcs8", keyPair.privateKey));
        const privateKey = pkcs8.slice(pkcs8.length - 32);
        return { publicKey, privateKey };
      } catch {
        const privJwk = await crypto.subtle.exportKey("jwk", keyPair.privateKey);
        if (!privJwk.d) {
          throw new Error("X25519 private key export missing 'd' parameter");
        }
        const privateKey = base64UrlToBytes(privJwk.d);
        return { publicKey, privateKey };
      }
    },

    async x25519(privateKey: Uint8Array, publicKey: Uint8Array): Promise<Uint8Array> {
      if (privateKey.length !== 32) {
        throw new Error(`X25519_KEY_INVALID: privateKey must be 32 bytes, got ${privateKey.length}`);
      }
      if (publicKey.length !== 32) {
        throw new Error(`X25519_KEY_INVALID: publicKey must be 32 bytes, got ${publicKey.length}`);
      }

      const pkcs8 = new Uint8Array(48);
      pkcs8.set(PKCS8_X25519_PREFIX, 0);
      pkcs8.set(privateKey, 16);

      let privCryptoKey: CryptoKey;
      try {
        privCryptoKey = await crypto.subtle.importKey(
          "pkcs8",
          pkcs8 as unknown as BufferSource,
          { name: "X25519" },
          false,
          ["deriveBits"],
        );
      } catch {
        const jwk = {
          kty: "OKP",
          crv: "X25519",
          d: bytesToBase64Url(privateKey),
        };
        privCryptoKey = await crypto.subtle.importKey(
          "jwk",
          jwk,
          { name: "X25519" },
          false,
          ["deriveBits"],
        );
      }

      const pubCryptoKey = await crypto.subtle.importKey(
        "raw",
        publicKey as unknown as BufferSource,
        { name: "X25519" },
        false,
        [],
      );

      const shared = await crypto.subtle.deriveBits(
        { name: "X25519", public: pubCryptoKey },
        privCryptoKey,
        256,
      );
      return new Uint8Array(shared);
    },

    async aeadEncrypt(
      key: Uint8Array,
      nonce: number,
      associatedData: Uint8Array,
      plaintext: Uint8Array,
    ): Promise<Uint8Array> {
      if (key.length !== 32) {
        throw new Error(`AEAD_KEY_INVALID: key must be 32 bytes, got ${key.length}`);
      }
      const nonceBytes = formatNonce(nonce);
      const cipher = chacha20poly1305(key, nonceBytes, associatedData);
      return cipher.encrypt(plaintext);
    },

    async aeadDecrypt(
      key: Uint8Array,
      nonce: number,
      associatedData: Uint8Array,
      ciphertext: Uint8Array,
    ): Promise<Uint8Array> {
      if (key.length !== 32) {
        throw new Error(`AEAD_KEY_INVALID: key must be 32 bytes, got ${key.length}`);
      }
      const nonceBytes = formatNonce(nonce);
      const cipher = chacha20poly1305(key, nonceBytes, associatedData);
      return cipher.decrypt(ciphertext);
    },

    async hash(data: Uint8Array): Promise<Uint8Array> {
      return blake2s(data);
    },

    async hkdf(ck: Uint8Array, ikm: Uint8Array): Promise<[Uint8Array, Uint8Array]> {
      const t = hmac(blake2s, ck, ikm);
      const o1 = hmac(blake2s, t, new Uint8Array([0x01]));
      const o2 = hmac(blake2s, t, concatBytes(o1, new Uint8Array([0x02])));
      return [o1, o2];
    },

    async hkdf3(ck: Uint8Array, ikm: Uint8Array): Promise<[Uint8Array, Uint8Array, Uint8Array]> {
      const t = hmac(blake2s, ck, ikm);
      const o1 = hmac(blake2s, t, new Uint8Array([0x01]));
      const o2 = hmac(blake2s, t, concatBytes(o1, new Uint8Array([0x02])));
      const o3 = hmac(blake2s, t, concatBytes(o2, new Uint8Array([0x03])));
      return [o1, o2, o3];
    },
  };
}
