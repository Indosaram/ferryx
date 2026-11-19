// macOS accessibility (AX) adapter for Q10's supervisor switch. Read-only except one AXPress.
//
//   osascript -l JavaScript mac-ax-supervisor-switch.js inspect <pid>
//   osascript -l JavaScript mac-ax-supervisor-switch.js press   <pid> '<expectation json>'
//
// It prints exactly one JSON object. `inspect` walks the AX tree of one pid and reports the nodes it
// saw with their identity fields, plus how far the walk got; the caller decides which node is the
// General supervisor switch and whether the walk covered the tree at all. `press` resolves the
// reported path again, re-reads every identity field, and refuses with the differences unless the
// node is still exactly what the caller matched - still enabled, still reading ON - and only then
// performs AXPress on it. No keyboard event, no mouse event, no focus change and no activation is
// ever sent; nothing outside that one element is touched.
//
// Bridge notes measured on maho-mac (macOS 27.2.1, 2026-11-19): AX attribute names must be passed
// through $() - a bare JS string answers kAXErrorIllegalArgument (-25201) - and an attribute value
// arrives as a bare CFTypeRef whose payload only reads through ObjC.castRefToObject(...).js, while a
// CFArray answers a numeric `count` property and objectAtIndex(i). ObjC.unwrap returns "[object
// Ref]" for both shapes and is not used here. A WKWebView's DOM id reaches AX as AXDOMIdentifier
// (AXIdentifier is normally null there), so both are read.

ObjC.import('ApplicationServices');
ObjC.import('Foundation');

// Identity fields in the order the JS side reports them, and the exact AX attribute each one is read
// from. WebKit's DOM-id attribute is `AXDOMIdentifier` - case-sensitive, and not the `AXIdentifier`
// that AppKit elements answer - so the mapping is spelled out instead of being derived from the
// field name: `'AX' + 'domIdentifier'` builds `AXDomIdentifier`, which no process answers.
const ATTRIBUTE_NAMES = {
  role: 'AXRole',
  subrole: 'AXSubrole',
  identifier: 'AXIdentifier',
  domIdentifier: 'AXDOMIdentifier',
  label: 'AXLabel',
  title: 'AXTitle',
  description: 'AXDescription',
  help: 'AXHelp',
};
const IDENTITY_FIELDS = Object.keys(ATTRIBUTE_NAMES);
// The product's Settings dialog nests several web groups below the web area; 14 levels stopped
// short of the General panel on a real WKWebView. The node bound keeps a runaway tree bounded.
const MAX_DEPTH = 24;
const MAX_NODES = 6000;

function copyAttr(el, name) {
  const ref = Ref();
  const err = $.AXUIElementCopyAttributeValue(el, $(name), ref);
  return err === 0 ? ref[0] : null;
}

/** The same read, with the AX error kept: a walk has to distinguish "this element has no children"
 * from "this element's children could not be read", because only the first leaves the tree covered. */
function copyAttrResult(el, name) {
  const ref = Ref();
  const err = $.AXUIElementCopyAttributeValue(el, $(name), ref);
  return { err, value: err === 0 ? ref[0] : null };
}

// kAXErrorAttributeUnsupported / kAXErrorNoValue: the element legitimately does not carry it.
const BENIGN_READ_ERRORS = new Set([-25205, -25212]);

function scalar(refVal) {
  if (refVal == null) return null;
  try {
    const v = ObjC.castRefToObject(refVal).js;
    return v === undefined ? null : v;
  } catch (e) {
    return null;
  }
}

function elements(refVal) {
  if (refVal == null) return [];
  const o = ObjC.castRefToObject(refVal);
  if (!o) return [];
  const n = Number(o.count);
  if (!Number.isFinite(n)) return [];
  const out = [];
  for (let i = 0; i < n; i += 1) {
    let el;
    try { el = o.objectAtIndex(i); } catch (e) { el = o[i]; }
    if (el != null) out.push(el);
  }
  return out;
}

/** Children of one element: AXChildren, or AXContents where an AppKit container answers only that.
 * `state` records any read that failed for a reason other than "unsupported" or "no value". */
function childElements(el, state = null) {
  const children = copyAttrResult(el, 'AXChildren');
  if (children.err === 0) return elements(children.value);
  if (state && !BENIGN_READ_ERRORS.has(children.err)) state.readErrors.push({ attribute: 'AXChildren', err: children.err });
  const contents = copyAttrResult(el, 'AXContents');
  if (contents.err === 0) return elements(contents.value);
  if (state && !BENIGN_READ_ERRORS.has(contents.err)) state.readErrors.push({ attribute: 'AXContents', err: contents.err });
  return [];
}

/** An attribute as a trimmed string, or null: AX answers "" for many attributes it does not set,
 * and a WKWebView does that routinely; an empty string is not an identity field. */
function identity(value) {
  if (value === null || value === undefined) return null;
  const s = String(value).trim();
  return s === '' ? null : s;
}

/** An AX boolean as a JS boolean, or null: AXEnabled can arrive as true/false, 1/0 or their strings. */
function bool(value) {
  if (value === true || value === 1) return true;
  if (value === false || value === 0) return false;
  const s = identity(value);
  if (s === null) return null;
  if (/^(1|true|yes|on)$/i.test(s)) return true;
  if (/^(0|false|no|off)$/i.test(s)) return false;
  return null;
}

/** The reported shape of one element: identity fields plus its live enabled/on state. */
function describe(el, path) {
  const get = (n) => scalar(copyAttr(el, n));
  const node = { path };
  for (const f of IDENTITY_FIELDS) {
    const v = identity(get(ATTRIBUTE_NAMES[f]));
    if (v !== null) node[f] = v;
  }
  node.enabled = bool(get('AXEnabled'));
  node.value = identity(get('AXValue'));
  return node;
}

function nodeAt(app, path) {
  let cur = app;
  for (const i of path) {
    const kids = childElements(cur);
    if (!(i >= 0 && i < kids.length)) return null;
    cur = kids[i];
  }
  return cur;
}

/**
 * Depth-first walk. `state` carries the coverage facts the caller needs: a walk that stopped at the
 * node or depth bound cannot prove the switch is unique, and a run that reports "no such switch"
 * must be able to say whether it actually looked everywhere it could.
 */
function walk(el, path, depth, out, state, roles) {
  if (out.length >= MAX_NODES) {
    state.truncated = true;
    return;
  }
  const roleRead = copyAttrResult(el, 'AXRole');
  const role = roleRead.err === 0 ? identity(scalar(roleRead.value)) : null;
  if (role == null) {
    // An element whose role could not be read at all is not the same as one that has none: the
    // subtree below it was never visited, so the walk cannot claim to have covered the tree.
    if (roleRead.err !== 0 && !BENIGN_READ_ERRORS.has(roleRead.err)) state.readErrors.push({ attribute: 'AXRole', err: roleRead.err });
    return;
  }
  out.push(describe(el, path));
  roles[role] = (roles[role] ?? 0) + 1;
  if (depth > state.maxDepth) state.maxDepth = depth;
  if (depth >= MAX_DEPTH) {
    // Only a cut with children below it leaves part of the tree unvisited.
    if (childElements(el, state).length > 0) state.depthLimited = true;
    return;
  }
  const kids = childElements(el, state);
  for (let i = 0; i < kids.length; i += 1) walk(kids[i], path.concat([i]), depth + 1, out, state, roles);
}

/**
 * Coverage of one walk. `complete` is true only when nothing was cut by a bound AND every attribute
 * read that mattered succeeded: a walk with an AX fault somewhere cannot prove the switch is unique
 * (or absent), so the caller must refuse rather than press on partial knowledge.
 */
function coverage(state) {
  const readErrors = state.readErrors.slice(0, 8);
  return {
    maxDepth: state.maxDepth,
    truncated: state.truncated === true,
    depthLimited: state.depthLimited === true,
    readErrors: { count: state.readErrors.length, seen: readErrors },
    complete: state.truncated !== true && state.depthLimited !== true && state.readErrors.length === 0,
  };
}

/** Every difference between what the caller matched and what the node says now. */
function differences(node, expectation) {
  const diffs = [];
  for (const f of IDENTITY_FIELDS) {
    const want = identity(expectation[f]);
    const got = identity(node[f]);
    if (want !== got) diffs.push(`${f}: expected ${JSON.stringify(want)}, got ${JSON.stringify(got)}`);
  }
  if (node.enabled !== true) diffs.push(`enabled: expected true, got ${JSON.stringify(node.enabled)}`);
  const value = identity(node.value);
  const on = value === '1' || value === 'true' || value === 'yes' || value === 'on';
  if (!on) diffs.push(`state: expected on, got ${JSON.stringify(node.value)}`);
  return diffs;
}

function run(argv) {
  const action = String(argv[0] || '');
  const pid = Number(argv[1]);
  if (!Number.isInteger(pid) || pid <= 0) return JSON.stringify({ ok: false, reason: `bad pid ${argv[1]}` });
  const trusted = Boolean($.AXIsProcessTrusted());
  if (!trusted) return JSON.stringify({ ok: false, trusted, pid, reason: 'this process is not trusted for accessibility' });
  const app = $.AXUIElementCreateApplication(pid);
  // A pid whose application does not answer AX at all is reported, never guessed around.
  const roleErr = $.AXUIElementCopyAttributeValue(app, $('AXRole'), Ref());
  if (roleErr !== 0) return JSON.stringify({ ok: false, trusted, pid, roleErr, reason: `AX did not answer for pid ${pid} (error ${roleErr})` });

  if (action === 'inspect') {
    const nodes = [];
    const state = { maxDepth: 0, truncated: false, depthLimited: false, readErrors: [] };
    const roles = Object.create(null);
    walk(app, [], 0, nodes, state, roles);
    return JSON.stringify({
      ok: true, trusted, pid, count: nodes.length,
      ...coverage(state),
      // A role histogram is what tells "the switch is below the cut" from "the web area exposed no
      // controls at all"; it carries no text from the tree.
      roles: Object.entries(roles).sort((a, b) => b[1] - a[1]).slice(0, 40),
      nodes,
    });
  }

  if (action === 'press') {
    let expectation;
    try { expectation = JSON.parse(String(argv[2] ?? '')); } catch (e) { return JSON.stringify({ ok: false, trusted, pid, reason: 'expectation is not JSON' }); }
    if (!Array.isArray(expectation.path)) return JSON.stringify({ ok: false, trusted, pid, reason: 'expectation carries no path' });
    const el = nodeAt(app, expectation.path);
    if (el == null) return JSON.stringify({ ok: false, trusted, pid, reason: 'the reported path no longer resolves' });
    const before = describe(el, expectation.path);
    const diffs = differences(before, expectation);
    if (diffs.length) return JSON.stringify({ ok: false, trusted, pid, reason: 'the node changed after it was matched; nothing was pressed', diffs, before });
    const pressError = $.AXUIElementPerformAction(el, $('AXPress'));
    return JSON.stringify({ ok: pressError === 0, trusted, pid, pressError, before, after: describe(el, expectation.path) });
  }

  return JSON.stringify({ ok: false, trusted, pid, reason: `unknown action ${JSON.stringify(action)}` });
}
