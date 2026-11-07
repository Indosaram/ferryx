// Login-form capture for the Ferryx password vault, run in an isolated world of the top frame
// only (the system webview's trusted-bridge world, or the Chromium engine's "ferryx-vault"
// world). Page script cannot reach this world's globals or its channel.
// `hooks` is the literal null in production; tests rewrite only that declaration.
(() => {
  const hooks = null;
  if (globalThis.__ferryxVaultCapture) return;
  let topFrame = false;
  try {
    topFrame = window.top === window;
  } catch (error) {
    topFrame = false;
  }
  if (!topFrame) return;
  const channel = () => {
    if (hooks) return hooks.channel();
    if (typeof __ferryxVault === 'function') return (body) => __ferryxVault(body);
    const world = globalThis.__ferryxWorld;
    if (world && typeof world.channel === 'function') return world.channel();
    return null;
  };
  const isTrusted = (event) => (hooks ? hooks.isTrusted(event) : event.isTrusted === true);
  const origin = () => (location.protocol === 'https:' || location.protocol === 'http:' ? location.origin : null);
  const visible = (input) => !input.disabled && input.type !== 'hidden';
  const usernameFor = (form, password) => {
    const fields = Array.from(form.querySelectorAll('input')).filter(visible);
    const before = fields.slice(0, fields.indexOf(password)).reverse();
    const candidate = before.find((input) => ['text', 'email', 'tel', ''].includes(input.type) || input.autocomplete === 'username')
      || fields.find((input) => input.autocomplete === 'username');
    return candidate ? String(candidate.value || '') : '';
  };
  let lastSent = '';
  const capture = (form) => {
    if (!form || typeof form.querySelectorAll !== 'function') return;
    const here = origin();
    if (!here) return;
    const passwords = Array.from(form.querySelectorAll('input[type="password"]')).filter(visible)
      .filter((input) => String(input.value || '').length > 0);
    if (passwords.length === 0) return;
    // A sign-up or change form has a new password last; a login form has one.
    const password = passwords[passwords.length - 1];
    const envelope = {
      v: 1,
      kind: 'formSubmitted',
      origin: here,
      username: usernameFor(form, passwords[0]).slice(0, 512),
      password: String(password.value).slice(0, 1024),
    };
    const body = JSON.stringify(envelope);
    if (body === lastSent) return;
    const send = channel();
    if (!send) return;
    lastSent = body;
    send(body);
  };
  document.addEventListener('submit', (event) => {
    if (!isTrusted(event)) return;
    capture(event.target);
  }, true);
  // Many sign-in pages never submit a form: they read the fields on a button click or Enter.
  document.addEventListener('click', (event) => {
    if (!isTrusted(event)) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const button = target.closest('button, input[type="submit"], input[type="button"], [role="button"]');
    if (!button) return;
    capture(button.closest('form') || document);
  }, true);
  document.addEventListener('keydown', (event) => {
    if (!isTrusted(event) || event.key !== 'Enter') return;
    const target = event.target;
    if (!(target instanceof HTMLInputElement)) return;
    capture(target.form || document);
  }, true);
  Object.defineProperty(globalThis, '__ferryxVaultCapture', { value: true, writable: false, configurable: false });
})();
