/* Password autofill for one saved login, run in an isolated world of the page (the system
   webview's trusted-bridge world, or the Chromium engine's "ferryx-vault" world). Ferryx puts
   {origin, username, password} in as escaped JSON (the one placeholder below).
   It fills nothing unless this is the top frame and its origin is exactly fill.origin, so a page
   that navigated away, or a cross-origin iframe, never receives the secret. It fills only fields
   the user can see, so a hidden form cannot collect a saved login. Values go through the native
   value setter and are announced with input/change events, as typing would.
   The Chromium engine protocol joins the script onto one line: block comments only. */
(() => {
  const fill = __FERRYX_FILL__;
  let topFrame = false;
  try { topFrame = window.top === window; } catch (error) { topFrame = false; }
  if (!topFrame) return 'not-top';
  if (location.origin !== fill.origin) return 'origin-mismatch';
  const usable = (input) => !input.disabled && input.type !== 'hidden' && !input.readOnly;
  const shown = (input) => {
    if (!input.isConnected) return false;
    if (typeof input.checkVisibility === 'function') {
      if (!input.checkVisibility({ opacityProperty: true, visibilityProperty: true })) return false;
      const box = input.getBoundingClientRect();
      if (box.width < 1 || box.height < 1) return false;
      if (box.right + window.scrollX <= 0 || box.bottom + window.scrollY <= 0) return false;
    }
    for (let node = input; node; node = node.parentElement) {
      if (node.hidden || node.inert || node.hasAttribute('inert')) return false;
      const style = getComputedStyle(node);
      if (style.display === 'none') return false;
      if (style.opacity !== '' && Number(style.opacity) === 0) return false;
    }
    const visibility = getComputedStyle(input).visibility;
    return visibility !== 'hidden' && visibility !== 'collapse';
  };
  const candidates = Array.from(document.querySelectorAll('input[type="password"]')).filter(usable);
  const passwords = candidates.filter(shown);
  if (candidates.length > 0 && passwords.length === 0) return 'not-visible';
  const password = passwords.find((input) => input.autocomplete !== 'new-password') || passwords[0];
  const scope = (password && password.form) || document;
  const fields = Array.from(scope.querySelectorAll('input')).filter((input) => usable(input) && shown(input));
  const before = password ? fields.slice(0, fields.indexOf(password)).reverse() : fields;
  const username = fields.find((input) => input.autocomplete === 'username')
    || before.find((input) => ['text', 'email', 'tel', ''].includes(input.type));
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
  const put = (input, value) => {
    input.focus();
    setter.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  };
  if (username && fill.username) put(username, fill.username);
  if (password) put(password, fill.password);
  return password ? 'filled' : (username ? 'username-only' : 'no-fields');
})()
