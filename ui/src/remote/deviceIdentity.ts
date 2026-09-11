/**
 * Derives a readable device name suggestion from the User Agent.
 * e.g. "iPhone - Safari", "macOS - Chrome", "Windows - Edge"
 */
export function suggestDeviceName(
  userAgent: string = typeof navigator !== "undefined" ? navigator.userAgent : "",
): string {
  const ua = userAgent;

  let os = "";
  if (/iPhone/i.test(ua)) {
    os = "iPhone";
  } else if (/iPad/i.test(ua) || (/Macintosh/i.test(ua) && typeof navigator !== "undefined" && navigator.maxTouchPoints > 1)) {
    os = "iPad";
  } else if (/Android/i.test(ua)) {
    os = "Android";
  } else if (/Mac OS X|Macintosh/i.test(ua)) {
    os = "macOS";
  } else if (/Windows|Win32|Win64/i.test(ua)) {
    os = "Windows";
  } else if (/Linux/i.test(ua)) {
    os = "Linux";
  }

  let browser = "";
  if (/Edg([A-Za-z0-9]+)?\//i.test(ua)) {
    browser = "Edge";
  } else if (/CriOS\//i.test(ua)) {
    browser = "Chrome";
  } else if (/FxiOS\//i.test(ua)) {
    browser = "Firefox";
  } else if (/Chrome\//i.test(ua)) {
    browser = "Chrome";
  } else if (/Firefox\//i.test(ua)) {
    browser = "Firefox";
  } else if (/Safari\//i.test(ua)) {
    browser = "Safari";
  } else if (/OPR\/|Opera/i.test(ua)) {
    browser = "Opera";
  }

  if (os && browser) {
    return `${os} - ${browser}`;
  }
  if (os) {
    return `${os} Device`;
  }
  if (browser) {
    return `${browser} Device`;
  }
  return /Mobile/i.test(ua) ? "Mobile Device" : "Browser Device";
}
