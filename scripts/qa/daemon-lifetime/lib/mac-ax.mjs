// Targeted macOS accessibility (AX) press for Q10's supervisor switch.
//
// Q10 on macOS used to open Settings and then walk Tab/Space through a guessed number of controls
// (SUPERVISOR_SWITCH_TABS plus a few probes). Run 20261119t082333z on 32b4d9a opened Settings,
// proved the QA window owned the foreground, and still left the opt-out marker absent with
// RunAtLoad true: owning the window says nothing about which control received Space. The General
// panel's switch has a stable identity (ui/src/components/settings/GeneralSection.tsx:490-491, id
// general-daemon-supervisor, aria-label "Restart background service automatically"), so the macOS
// path asks accessibility for that one element and presses it. It sends no Tab, no Space and no
// other key at the desktop, and it never falls back to them.
//
// Division of labour: fixtures/mac-ax-supervisor-switch.js does only the ObjC plumbing - it walks
// the AX tree of one pid and reports the nodes it saw, or presses one exact path it re-verified
// against the expectation given to it. Every decision (is this the switch? is it unique, enabled,
// in the expected state? was the walk complete?) is made here, where it is unit-tested against
// synthetic trees.

import { readFileSync } from 'node:fs';

/** DOM id of the General supervisor switch (GeneralSection.tsx:490). */
export const SUPERVISOR_SWITCH_IDENTIFIER = 'general-daemon-supervisor';
/** aria-label of the same switch (GeneralSection.tsx:491). */
export const SUPERVISOR_SWITCH_LABEL = 'Restart background service automatically';

/**
 * Roles a Radix switch can reach accessibility as. WebKit maps role="switch" to a checkbox
 * (AXCheckBox) and newer WebKit also answers subrole AXSwitch; a native NSSwitchButton is AXCheckBox
 * too. Plain AXButton is deliberately absent: a button that merely carries the label is not a switch
 * and pressing it is not the product behaviour under test.
 */
export const SWITCH_ROLES = Object.freeze(['AXSwitch', 'AXCheckBox']);

/** Barrier kind recorded when the switch could not be identified or pressed. Instrumentation, not a product verdict. */
export const MAC_AX_SWITCH_UNCONFIRMED = 'MAC_SUPERVISOR_SWITCH_UNCONFIRMED';

/** Where the fixture is written inside the run dir; the reviewed source is shipped verbatim. */
export const MAC_AX_FIXTURE_FILE = 'mac-ax-supervisor-switch.js';

/** How long the rendered switch may take to appear after the settings receipt, and to settle after one press. */
export const MAC_AX_INSPECT_DEADLINE_MS = 20_000;
export const MAC_AX_STATE_DEADLINE_MS = 10_000;

/** The reviewed fixture source, shipped to the host and executed there exactly as written. */
export function axFixtureScript() {
  return readFileSync(new URL('../fixtures/mac-ax-supervisor-switch.js', import.meta.url), 'utf8');
}

function text(value) {
  if (value === null || value === undefined) return null;
  if (typeof value === 'object') return null;
  const s = String(value).trim();
  return s === '' ? null : s;
}

/** Accessibility booleans arrive as JS booleans, 0/1 numbers or the strings of either. */
export function axBool(value) {
  if (value === true || value === 1) return true;
  if (value === false || value === 0) return false;
  const s = text(value);
  if (s === null) return null;
  if (/^(1|true|yes|on)$/i.test(s)) return true;
  if (/^(0|false|no|off)$/i.test(s)) return false;
  return null;
}

/** One reported AX node as a plain record, or null when it carries no role at all. */
export function normalizeAxNode(raw) {
  if (!raw || typeof raw !== 'object') return null;
  const role = text(raw.role);
  if (!role) return null;
  return {
    path: Array.isArray(raw.path) ? raw.path : null,
    role,
    subrole: text(raw.subrole),
    // AXDOMIdentifier is the WebKit attribute behind a DOM id; AXIdentifier is usually null inside
    // a WKWebView, so both spellings are read and either one may carry the product's id.
    identifier: text(raw.identifier),
    domIdentifier: text(raw.domIdentifier),
    label: text(raw.label),
    title: text(raw.title),
    description: text(raw.description),
    help: text(raw.help),
    enabled: axBool(raw.enabled),
    state: axBool(raw.value),
  };
}

const switchShaped = (n) => SWITCH_ROLES.includes(n.role) || n.subrole === 'AXSwitch';

/**
 * Which identity fields of one node carry the supervisor identity.
 *
 * The DOM id is matched on either AX spelling (`AXIdentifier` on AppKit, `AXDOMIdentifier` in a
 * WKWebView). The accessible name is label/title/description - what a screen reader announces as the
 * control's name. `help` is deliberately NOT one of them: it is a tooltip-style hint, and a node
 * whose only supervisor string is help text has not been named as the supervisor switch.
 */
export function identityFields(n) {
  const out = [];
  if (n.identifier === SUPERVISOR_SWITCH_IDENTIFIER || n.domIdentifier === SUPERVISOR_SWITCH_IDENTIFIER) out.push('identifier');
  if ([n.label, n.title, n.description].some((v) => v === SUPERVISOR_SWITCH_LABEL)) out.push('label');
  return out;
}

/**
 * Every switch-shaped node that carries the supervisor identity, by DOM id or by its accessible
 * label. Matching happens on the reported identity fields only - never on position, never on "the
 * nth switch" - so a General panel whose layout shifted cannot silently nominate a neighbour.
 */
export function supervisorSwitchCandidates(nodes) {
  const out = [];
  for (const raw of nodes ?? []) {
    const n = normalizeAxNode(raw);
    if (!n || !switchShaped(n)) continue;
    const fields = identityFields(n);
    if (fields.length === 0) continue;
    out.push({ ...n, byIdentifier: fields.includes('identifier'), byLabel: fields.includes('label') });
  }
  return out;
}

/** The bounded, JSON-safe view of a candidate kept in the receipt. */
export function summarizeCandidate(c) {
  return {
    path: c.path, role: c.role, subrole: c.subrole, identifier: c.identifier, domIdentifier: c.domIdentifier,
    label: c.label, title: c.title, description: c.description, help: c.help,
    enabled: c.enabled, state: c.state, byIdentifier: c.byIdentifier, byLabel: c.byLabel,
  };
}

/**
 * Picks the one switch Q10 may press, or explains why it may press none.
 *
 * `requireState` is the "never turn it back on" rule: Q10 exists to opt OUT, so the press is
 * authorised only while the switch reads as ON. A switch already OFF (an earlier attempt, a user
 * change, a product that inverted the API) is reported and left alone - pressing it would enable the
 * supervisor again, which is the opposite of what Q10 asserts. `{ requireState: false }` lets the
 * same selector state a candidate without demanding it be on (used by the post-press read).
 *
 * `traversal` is the walk's own coverage record, and it must be present and explicitly complete:
 * an unreadable, unparsed or hard-failed walk reports nothing this function can trust, so a missing
 * `complete: true` refuses exactly like `complete: false` does. Absence of evidence is never a
 * licence to press.
 */
export function selectSupervisorSwitch(nodes, { requireState = true, traversal = null } = {}) {
  const candidates = supervisorSwitchCandidates(nodes);
  const report = { candidates: candidates.map(summarizeCandidate) };
  if (!traversal || traversal.complete !== true) {
    const why = traversal && traversal.readErrors ? ` (${traversal.readErrors.count ?? 'some'} AX reads failed)` : '';
    return { ok: false, reason: `the accessibility walk did not report a complete traversal${why}, so a unique match cannot be proved`, ...report };
  }
  if (candidates.length === 0) {
    return { ok: false, reason: 'no switch-shaped node carries the supervisor identity (general-daemon-supervisor / "Restart background service automatically")', ...report };
  }
  if (candidates.length > 1) {
    return { ok: false, reason: `ambiguous: ${candidates.length} switches carry the supervisor identity`, ...report };
  }
  const match = candidates[0];
  if (match.enabled !== true) return { ok: false, reason: 'the supervisor switch is not enabled (the product disables it while busy)', ...report };
  if (!Array.isArray(match.path) || match.path.length === 0) return { ok: false, reason: 'the matched node was reported without a path (an empty path is the application itself)', ...report };
  if (match.state === null) return { ok: false, reason: 'the supervisor switch reported no readable on/off state', ...report };
  if (requireState && match.state !== true) {
    return { ok: false, reason: 'the supervisor switch already reads OFF: pressing it would turn the supervisor back on', ...report };
  }
  return { ok: true, match, ...report };
}

/**
 * What the fixture must re-verify before it presses: the exact node it reported, by path and by every
 * identity field it compared, still enabled and still reading ON. The field set is the same one the
 * fixture's `differences` checks - role/subrole/identifier/domIdentifier/label/title/description/help -
 * so the two sides cannot drift into checking (or skipping) different attributes. A field the tree
 * could not answer is pinned as null, which the fixture compares against null; it never becomes an
 * accidental mismatch that would block an otherwise correct press, and it never becomes a skipped
 * check either.
 */
export function pressExpectation(match) {
  return {
    path: match.path,
    role: match.role,
    subrole: match.subrole ?? null,
    identifier: match.identifier ?? null,
    domIdentifier: match.domIdentifier ?? null,
    label: match.label ?? null,
    title: match.title ?? null,
    description: match.description ?? null,
    help: match.help ?? null,
    state: true,
    enabled: true,
  };
}

/**
 * The bounded diagnostic view of an unresolved inspect: how far the walk got, whether it covered the
 * tree, the role histogram, and - only for switch-shaped nodes and nodes carrying the identity -
 * role, subrole and state. No text from the tree, and nothing of another application: a receipt
 * that cannot identify the switch still says what this run-owned GUI exposed, without dumping it.
 */
export function axDiagnostics(seen) {
  const list = (seen?.nodes ?? []).map(normalizeAxNode).filter(Boolean);
  const shaped = list.filter(switchShaped);
  const identity = list.filter((n) => identityFields(n).length > 0);
  const LIMIT = 12;
  const view = (n) => ({ role: n.role, subrole: n.subrole, enabled: n.enabled, state: n.state, matchedBy: identityFields(n) });
  return {
    nodes: list.length,
    maxDepth: seen?.maxDepth ?? null,
    truncated: seen?.truncated ?? null,
    depthLimited: seen?.depthLimited ?? null,
    complete: seen?.complete ?? null,
    readErrors: seen?.readErrors ?? null,
    roles: Array.isArray(seen?.roles) ? seen.roles.slice(0, 40).map(([role, count]) => ({ role, count })) : null,
    switchShaped: { count: shaped.length, seen: shaped.slice(0, LIMIT).map(view) },
    identity: { count: identity.length, seen: identity.slice(0, LIMIT).map(view) },
  };
}
