// Evidence redaction for the supervisor QA harnesses.
//
// Evidence files outlive the run and get copied into reports, so fixture credentials (pairing
// codes, bearer tokens, socket tickets, subscribe secrets) are still credentials: they never
// reach a file or an error message. The live values stay in memory only.

const SECRET_KEYS = /^(?:code|token|ticket|secret|pin|pairingCode|pairing_code|accessToken|access_token|refreshToken|refresh_token|authToken|auth_token|subscriptionToken|vapidPrivateKey|vapid_private_key)$/i;
const BEARER = /^(?:Bearer|bearer)\s+\S+/;

/** Returns a deep copy with secret-shaped values replaced by `[redacted]`. */
export const redact = (value, depth = 0) => {
  if (depth > 12 || value === null || typeof value !== 'object') return value;
  if (Array.isArray(value)) return value.map((item) => redact(item, depth + 1));
  const clean = {};
  for (const [key, item] of Object.entries(value)) {
    const secretKey = SECRET_KEYS.test(key);
    if (secretKey && (item === null || typeof item !== 'object')) {
      clean[key] = typeof item === 'string' && item ? '[redacted]' : item;
    } else if (typeof item === 'string' && BEARER.test(item)) {
      clean[key] = '[redacted]';
    } else {
      clean[key] = redact(item, depth + 1);
    }
  }
  return clean;
};
