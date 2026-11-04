// crypto.randomUUID polyfill. Plain-http LAN origins are not secure contexts, so browsers
// omit randomUUID there. Loaded as a classic script before the app bundle because the
// served Content-Security-Policy (script-src 'self') blocks inline scripts.
if (typeof globalThis !== "undefined" && (!globalThis.crypto || typeof globalThis.crypto.randomUUID !== "function")) {
  (function() {
    try {
      if (!globalThis.crypto) {
        try { Object.defineProperty(globalThis, "crypto", { value: {}, writable: true, configurable: true }); }
        catch(e) { globalThis.crypto = {}; }
      }
      if (typeof globalThis.crypto.randomUUID !== "function") {
        var hex = [];
        for (var i = 0; i < 256; i++) hex[i] = (i < 16 ? "0" : "") + i.toString(16);
        var fallbackUUID = function() {
          var b = new Uint8Array(16);
          if (typeof globalThis.crypto.getRandomValues === "function") {
            try { globalThis.crypto.getRandomValues(b); }
            catch(e) { for (var j = 0; j < 16; j++) b[j] = Math.floor(Math.random() * 256); }
          } else {
            for (var j = 0; j < 16; j++) b[j] = Math.floor(Math.random() * 256);
          }
          b[6] = (b[6] & 0x0f) | 0x40;
          b[8] = (b[8] & 0x3f) | 0x80;
          return (
            hex[b[0]] + hex[b[1]] + hex[b[2]] + hex[b[3]] + "-" +
            hex[b[4]] + hex[b[5]] + "-" +
            hex[b[6]] + hex[b[7]] + "-" +
            hex[b[8]] + hex[b[9]] + "-" +
            hex[b[10]] + hex[b[11]] + hex[b[12]] + hex[b[13]] + hex[b[14]] + hex[b[15]]
          );
        };
        try { Object.defineProperty(globalThis.crypto, "randomUUID", { value: fallbackUUID, writable: true, configurable: true }); }
        catch(e) { globalThis.crypto.randomUUID = fallbackUUID; }
      }
    } catch(e) {}
  })();
}
