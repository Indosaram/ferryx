// The macOS Q10 switch contract: the supervisor switch is found by identity, pressed only while it
// reads ON and only after a provably complete accessibility walk, and never walked into with
// Tab/Space.
//
// Measured on product 32b4d9a (run 20261119t082333z, hosted macOS): Q10 opened Settings, proved the
// QA window owned the foreground, and still left the opt-out marker absent with RunAtLoad true. The
// foreground proof says which window took the key, not which control took it. These tests pin the
// replacement - a targeted accessibility press on the one element carrying the supervisor identity -
// against synthetic AX trees. No host, GUI, daemon or real desktop is touched.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
  SUPERVISOR_SWITCH_IDENTIFIER, SUPERVISOR_SWITCH_LABEL, SWITCH_ROLES,
  axBool, normalizeAxNode, identityFields, supervisorSwitchCandidates, summarizeCandidate,
  selectSupervisorSwitch, pressExpectation, axFixtureScript, axDiagnostics, MAC_AX_SWITCH_UNCONFIRMED,
} from './lib/mac-ax.mjs';
import { block, STATUSES } from './lib/summary.mjs';
import { parseLaunchAgentPlist, demonstrablyUnregistered } from './scenarios/q10-q13.mjs';

const dir = new URL('./', import.meta.url);
const READ = (rel) => readFileSync(new URL(rel, dir), 'utf8');

/** A General-panel shaped tree: a neighbour switch first, then the supervisor switch. */
function generalPanel({ identifier = SUPERVISOR_SWITCH_IDENTIFIER, label = SUPERVISOR_SWITCH_LABEL, value = '1', enabled = true, role = 'AXCheckBox' } = {}) {
  return [
    { path: [], role: 'AXApplication', subrole: null, identifier: null, label: null, title: 'fixture', enabled: null, value: null },
    { path: [0], role: 'AXWindow', subrole: 'AXStandardWindow', identifier: null, label: null, title: 'Settings', enabled: null, value: null },
    { path: [0, 0], role: 'AXCheckBox', subrole: null, identifier: 'general-advanced-features', label: null, title: 'Advanced features', description: 'Advanced features', enabled: true, value: '0' },
    { path: [0, 1], role, subrole: role === 'AXSwitch' ? 'AXSwitch' : null, identifier, label: null, title: label, description: label, enabled, value },
  ];
}

/** A walk the fixture could vouch for. */
const COMPLETE = { complete: true };

/** The identity attributes the fixture reads, in the fixture's own spelling. */
function fixtureAttributeMap() {
  const source = axFixtureScript();
  const block = /const ATTRIBUTE_NAMES = \{([\s\S]*?)\};/.exec(source);
  assert.ok(block, 'the fixture must state its attribute names');
  const map = {};
  for (const m of block[1].matchAll(/(\w+):\s*'([^']+)'/g)) map[m[1]] = m[2];
  return map;
}

test('the identity is the product\'s own, and the DOM id is read through the real AXDOMIdentifier attribute', () => {
  const source = READ('../../../ui/src/components/settings/GeneralSection.tsx');
  assert.match(source, new RegExp(`id="${SUPERVISOR_SWITCH_IDENTIFIER}"`), 'the DOM id the selector matches must exist in the product');
  assert.match(source, new RegExp(`aria-label="${SUPERVISOR_SWITCH_LABEL}"`), 'the accessible label the selector matches must exist in the product');
  assert.deepEqual([...SWITCH_ROLES], ['AXSwitch', 'AXCheckBox'], 'only switch-shaped roles can be nominated');

  // WebKit's attribute is case-sensitive AXDOMIdentifier. It is not derivable by capitalizing the
  // field name (that builds AXDomIdentifier) and it is not AXIdentifier (null inside a WKWebView),
  // so the fixture maps every field explicitly and this pins the one that matters.
  const attributes = fixtureAttributeMap();
  assert.equal(attributes.domIdentifier, 'AXDOMIdentifier');
  assert.equal(attributes.identifier, 'AXIdentifier');
  const sourceText = axFixtureScript();
  assert.doesNotMatch(sourceText, /get\('AX' \+/, 'attribute names are mapped, not built from the field name');
  for (const field of Object.keys(attributes)) assert.ok(sourceText.includes(attributes[field]), `the fixture must read ${field} from ${attributes[field]}`);
});

test('the switch is found by identity, beside a neighbour it must not pick', () => {
  const found = supervisorSwitchCandidates(generalPanel());
  assert.equal(found.length, 1, JSON.stringify(found));
  assert.equal(found[0].path.join('.'), '0.1');
  assert.equal(found[0].byIdentifier, true);
  assert.equal(found[0].byLabel, true);
  assert.equal(selectSupervisorSwitch(generalPanel(), { traversal: COMPLETE }).ok, true);
});

test('the DOM id alone and the accessible name alone each identify the switch', () => {
  // Radix renders aria-label on the control; WebKit sometimes reports only one of the two, and
  // AXIdentifier is usually null inside a WKWebView, so either identity must carry the match.
  const byDomId = generalPanel({ identifier: null, label: null });
  byDomId[3].domIdentifier = SUPERVISOR_SWITCH_IDENTIFIER;
  const foundDom = supervisorSwitchCandidates(byDomId);
  assert.equal(foundDom.length, 1, JSON.stringify(foundDom));
  assert.equal(foundDom[0].byIdentifier, true);
  assert.equal(foundDom[0].byLabel, false);

  const byLabelOnly = generalPanel({ identifier: null });
  const foundLabel = supervisorSwitchCandidates(byLabelOnly);
  assert.equal(foundLabel.length, 1, JSON.stringify(foundLabel));
  assert.equal(foundLabel[0].byIdentifier, false);
  assert.equal(foundLabel[0].byLabel, true);
});

test('help text alone is not an accessible name, and a label on a non-switch role is not the switch', () => {
  const helpOnly = generalPanel({ identifier: null, label: null, value: '1' });
  helpOnly[3].title = 'Something else';
  helpOnly[3].description = 'Something else';
  helpOnly[3].help = SUPERVISOR_SWITCH_LABEL;
  assert.deepEqual(supervisorSwitchCandidates(helpOnly), [], 'a tooltip hint does not name the control');
  assert.deepEqual(identityFields(normalizeAxNode(helpOnly[3])), []);

  const tree = [
    { path: [0], role: 'AXCheckBox', identifier: 'general-advanced-features', title: 'Advanced features', enabled: true, value: '0' },
    { path: [1], role: 'AXButton', identifier: SUPERVISOR_SWITCH_IDENTIFIER, title: SUPERVISOR_SWITCH_LABEL, enabled: true, value: null },
    { path: [2], role: 'AXStaticText', identifier: null, title: SUPERVISOR_SWITCH_LABEL, enabled: null, value: null },
    { path: [3], role: 'AXCheckBox', identifier: 'general-advanced-features', title: SUPERVISOR_SWITCH_LABEL, enabled: true, value: '1' },
  ];
  assert.deepEqual(supervisorSwitchCandidates(tree).map((c) => c.path.join('.')), ['3'], 'only a switch-shaped node carrying the text is a candidate');
});

test('no match, several matches, a disabled switch and no readable state are each refused with what was seen', () => {
  const none = selectSupervisorSwitch([{ path: [0], role: 'AXCheckBox', identifier: 'general-advanced-features', title: 'Advanced features', enabled: true, value: '0' }], { traversal: COMPLETE });
  assert.equal(none.ok, false);
  assert.deepEqual(none.candidates, [], 'a refusal says the identity was absent, so nothing was pressed');

  const dupeTree = [...generalPanel(), { path: [0, 2], role: 'AXCheckBox', identifier: SUPERVISOR_SWITCH_IDENTIFIER, title: SUPERVISOR_SWITCH_LABEL, enabled: true, value: '1' }];
  const dupe = selectSupervisorSwitch(dupeTree, { traversal: COMPLETE });
  assert.equal(dupe.ok, false);
  assert.equal(dupe.candidates.length, 2, 'an ambiguous tree reports every candidate so the receipt shows why');
  assert.equal(dupe.match, undefined, 'nothing is nominated out of an ambiguity');

  const off = selectSupervisorSwitch(generalPanel({ enabled: false }), { traversal: COMPLETE });
  assert.equal(off.ok, false);
  assert.equal(off.candidates[0].enabled, false);

  const unknown = selectSupervisorSwitch(generalPanel({ value: 'mixed' }), { traversal: COMPLETE });
  assert.equal(unknown.ok, false);
  assert.equal(unknown.match, undefined, 'an unreadable state is never guessed');
});

test('a switch that already reads OFF is never pressed: that would turn the supervisor back on', () => {
  const off = selectSupervisorSwitch(generalPanel({ value: '0' }), { traversal: COMPLETE });
  assert.equal(off.ok, false);
  assert.match(off.reason, /already reads OFF/);
  assert.equal(off.candidates.length, 1, 'the candidate is reported, only the press is refused');
  // The same tree is still readable when the caller only wants to state it (requireState:false).
  const stated = selectSupervisorSwitch(generalPanel({ value: '0' }), { requireState: false, traversal: COMPLETE });
  assert.equal(stated.ok, true);
  assert.equal(stated.match.state, false);
});

test('a traversal that is cut off, unreadable, hard-failed or merely absent never authorizes a press', () => {
  const nodes = generalPanel(); // the switch is right there and reads ON
  const refusals = {
    'cut off': { complete: false, truncated: true },
    'depth limited': { complete: false, depthLimited: true },
    'read errors': { complete: false, readErrors: { count: 2, seen: [{ attribute: 'AXChildren', err: -25204 }] } },
    missing: null,
    empty: {},
    unknown: { complete: null },
    'wrong type': { complete: 'true' },
  };
  for (const [label, traversal] of Object.entries(refusals)) {
    const r = selectSupervisorSwitch(nodes, { traversal });
    assert.equal(r.ok, false, `${label} traversal must not authorize a press`);
    assert.equal(r.match, undefined, `${label} traversal nominates nothing`);
  }
  // Only an explicit complete:true gets through.
  assert.equal(selectSupervisorSwitch(nodes, { traversal: { complete: true } }).ok, true);
});

test('the press expectation pins the exact node and every attribute the fixture re-checks', () => {
  const match = selectSupervisorSwitch(generalPanel(), { traversal: COMPLETE }).match;
  const expectation = pressExpectation(match);
  assert.deepEqual(expectation.path, [0, 1]);
  assert.equal(expectation.identifier, SUPERVISOR_SWITCH_IDENTIFIER);
  assert.equal(expectation.title, SUPERVISOR_SWITCH_LABEL);
  assert.equal(expectation.enabled, true);
  assert.equal(expectation.state, true, 'the fixture refuses to press a node that no longer reads ON');

  // Field parity with the fixture's `differences`: it compares every field in IDENTITY_FIELDS, so an
  // expectation that drops one leaves that attribute unverified on the second read.
  const fixtureFields = [.../const IDENTITY_FIELDS = Object\.keys\(ATTRIBUTE_NAMES\);/.test(axFixtureScript())
    ? Object.keys(fixtureAttributeMap())
    : []];
  assert.ok(fixtureFields.length > 0, 'the fixture states its identity fields');
  const pinned = Object.keys(expectation).filter((k) => !['path', 'state', 'enabled'].includes(k));
  assert.deepEqual(pinned.sort(), fixtureFields.sort(), 'every attribute the fixture compares is pinned by the expectation');
  for (const f of fixtureFields) assert.equal(expectation[f] ?? null, match[f] ?? null, `${f} is pinned to the matched value`);
});

test('accessibility scalars are read the way the bridge reports them, and a junk node is dropped', () => {
  assert.equal(axBool(true), true);
  assert.equal(axBool(1), true);
  assert.equal(axBool('1'), true);
  assert.equal(axBool('true'), true);
  assert.equal(axBool(0), false);
  assert.equal(axBool('0'), false);
  assert.equal(axBool('off'), false);
  assert.equal(axBool(null), null);
  assert.equal(axBool(''), null);
  assert.equal(axBool('maybe'), null, 'an unreadable state is never guessed');
  assert.equal(axBool({ nested: true }), null, 'an object payload is not a boolean');

  assert.equal(normalizeAxNode(null), null);
  assert.equal(normalizeAxNode({ role: null }), null, 'a node without a role carries no identity');
  const node = normalizeAxNode({ path: [1], role: 'AXCheckBox', title: '  ', identifier: ' x ', enabled: '1', value: '0' });
  assert.equal(node.title, null, 'whitespace is not a title');
  assert.equal(node.identifier, 'x');
  assert.equal(node.enabled, true);
  assert.equal(node.state, false);

  const summary = summarizeCandidate({ ...node, byIdentifier: true, byLabel: false });
  assert.deepEqual(Object.keys(summary).sort(), ['byIdentifier', 'byLabel', 'description', 'domIdentifier', 'enabled', 'help', 'identifier', 'label', 'path', 'role', 'state', 'subrole', 'title']);
});

test('the diagnostics of an unresolved tree stay bounded and carry no text', () => {
  const tree = [
    ...generalPanel({ identifier: null }),
    { path: [0, 2], role: 'AXButton', title: 'Some private window title', label: 'secret label', enabled: true, value: null },
    { path: [0, 3], role: 'AXCheckBox', title: 'Another switch', enabled: true, value: '1' },
  ];
  const diag = axDiagnostics({ nodes: tree, count: tree.length, maxDepth: 3, complete: false, depthLimited: true, readErrors: { count: 0, seen: [] }, roles: [['AXCheckBox', 3]] });
  assert.equal(diag.nodes, tree.length);
  assert.equal(diag.complete, false);
  assert.equal(diag.depthLimited, true);
  assert.equal(diag.switchShaped.count, 3, 'both panel switches and the additional switch are counted');
  assert.equal(diag.identity.count, 1, 'only the node carrying the identity is listed as such');
  // No label/title text of any node reaches the receipt.
  assert.doesNotMatch(JSON.stringify(diag), /secret label|private window title|Another switch/);
  for (const entry of [...diag.switchShaped.seen, ...diag.identity.seen]) {
    assert.deepEqual(Object.keys(entry).sort(), ['enabled', 'matchedBy', 'role', 'state', 'subrole']);
  }
  assert.deepEqual(axDiagnostics(null).nodes, 0);
});

test('the shipped fixture is the reviewed source, reads AXDOMIdentifier, and can only send AXPress', () => {
  const source = axFixtureScript();
  assert.equal(source, READ('./fixtures/mac-ax-supervisor-switch.js'), 'the loader ships the file verbatim');
  assert.match(source, /AXUIElementCopyAttributeValue/);
  assert.match(source, /'inspect'/);
  assert.match(source, /'press'/);
  assert.match(source, /\$\('AXPress'\)/, 'the only action the fixture can send is AXPress on the verified element');
  assert.match(source, /'AXDOMIdentifier'/);
  assert.doesNotMatch(source, /CGEvent|AXUIElementPostKeyboardEvent|keystroke|System Events/, 'the fixture never sends a key or a synthesized event');
  assert.doesNotMatch(source, /AXUIElementSetAttributeValue/, 'the fixture never writes an attribute, so it cannot turn a switch on');
  // An AX read that failed for a real reason is reported as incomplete coverage, not as "no children".
  assert.match(source, /state\.readErrors\.push/, 'a failed attribute read is recorded as incomplete coverage');
  assert.match(source, /complete: state\.truncated !== true && state\.depthLimited !== true && state\.readErrors\.length === 0/);
});

test('an unresolved switch is a barrier that names the observation, not a product failure', () => {
  assert.ok(STATUSES.includes('BLOCKED'));
  const result = { id: 'Q10', status: 'BLOCKED', assertions: [], evidence: [] };
  const selection = selectSupervisorSwitch(generalPanel(), { traversal: { complete: false } });
  const observation = { identity: { pid: 4242, registry: 'match', ok: true }, selection, press: null };
  block(result, MAC_AX_SWITCH_UNCONFIRMED, { scenario: 'Q10', at: 'mac-ax', ax: observation });
  assert.equal(result.status, 'BLOCKED');
  assert.equal(result.barrier.kind, 'MAC_SUPERVISOR_SWITCH_UNCONFIRMED');
  assert.equal(observation.selection.ok, false, 'the barrier carries the observation, so the reason is auditable');
});

test('Q10 routes macOS through the accessibility press and leaves the other platforms on the keyboard walk', () => {
  const src = READ('./scenarios/q10-q13.mjs');
  const q10 = src.slice(src.indexOf('export async function Q10('), src.indexOf('export async function Q10b('));
  assert.match(q10, /ctx\.platform === 'mac'\s*\?\s*await toggleAutostartOffOnMac\(/, 'macOS must take the AX path');
  assert.match(q10, /: await toggleAutostartOffThroughSettings\(/, 'the keyboard walk stays for Linux and Windows');
  // "Settings opened" and the marker stay the product's own receipts on every platform.
  assert.match(q10, /cmd_daemon_supervisor_get receipt/);
  assert.match(q10, /settings toggle wrote the opt-out marker \(product receipt\)/);

  const mac = src.slice(src.indexOf('async function toggleAutostartOffOnMac('), src.indexOf('async function waitAutostartRemoved('));
  assert.match(mac, /pressSupervisorSwitchByAccessibility\(ctx, \{ optout, gui, key \}\)/);
  assert.doesNotMatch(mac, /mahord\.key\(|mahord\.click\(|mahord\.hotkey\(|SUPERVISOR_SWITCH_TABS|SUPERVISOR_SWITCH_PROBES/, 'the macOS path sends no key, chord or click of its own');
  assert.match(mac, /openSettings\(ctx, logFile/, 'Settings is still opened by the product shortcut, gated on the focus proof');
  assert.match(src, /export const SUPERVISOR_SWITCH_TABS = 1 \+ 9 \+ 4;/, 'the walk constants remain for the platforms that use them');

  const ax = src.slice(src.indexOf('async function pressSupervisorSwitchByAccessibility('), src.indexOf('/** The macOS generation plist as read'));
  assert.match(ax, /ctx\.agent\.call\('check', \{ pid: gui\.pid \}\)/, 'the recorded pid identity is re-checked before AX touches it');
  assert.match(ax, /confirmInputOwnership\(ctx, gui, 'ax-press'\)/, 'the foreground is proved before the press, like every other input path');
  assert.match(ax, /selectSupervisorSwitch\(/, 'the decision is the tested selector, not an inline guess');
  assert.match(ax, /JSON\.stringify\(pressExpectation\(selection\.match\)\)/, 'the fixture is given the exact node to re-verify');
  assert.match(ax, /seen\.complete === true/, 'only an explicitly complete walk may proceed');
  assert.doesNotMatch(ax, /mahord\.(key|hotkey|click)/, 'the AX path sends no desktop input at all');
  assert.doesNotMatch(ax, /writeFile[^\n]*optout|optout[^\n]*writeFile/, 'the marker stays the product\'s own receipt');
  assert.match(ax, /observeSwitchState\(/, 'the post-press state is observed, not assumed');
});

if (process.platform === 'darwin') test('the native plist parser requires the expected label and explicit false launch conditions', () => {
  const label = 'ferryx-qa-20261119t082333z.a';
  const rendered = `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>ferryx-qa-20261119t082333z.a</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
</dict>
</plist>
`;
  const registered = parseLaunchAgentPlist(rendered, label);
  assert.deepEqual([registered.present, registered.parsed, registered.complete, registered.runAtLoad], [true, true, true, true]);
  assert.equal(registered.label, 'ferryx-qa-20261119t082333z.a');
  assert.equal(demonstrablyUnregistered(registered), false);

  const stillKeptAlive = rendered.replace('<key>RunAtLoad</key>\n    <true/>', '<key>RunAtLoad</key>\n    <false/>');
  assert.equal(demonstrablyUnregistered(parseLaunchAgentPlist(stillKeptAlive, label)), false);
  const optedOut = stillKeptAlive.replace(/<dict>\s*<key>SuccessfulExit<\/key>\s*<false\/>\s*<\/dict>/, '<false/>');
  const disabled = parseLaunchAgentPlist(optedOut, label);
  assert.equal(demonstrablyUnregistered(parseLaunchAgentPlist(optedOut, 'not-the-QA-label')), false);
  assert.deepEqual([disabled.complete, disabled.runAtLoad], [true, false]);
  assert.equal(demonstrablyUnregistered(disabled), true);

  // The three degenerate readings all say "no autostart", which is exactly why none of them may be
  // accepted as proof that the product removed it.
  for (const [kind, text] of [['missing', null], ['empty', ''], ['blank', '  \n\t'], ['malformed', '<plist><dict><key>RunAtLoad</key>'], ['mismatched tags', optedOut.replace('</dict>', '</array>')], ['no RunAtLoad', rendered.replace(/<key>RunAtLoad<\/key>\s*<true\/>/, '')], ['truncated', rendered.slice(0, 200)]]) {
    const parsed = parseLaunchAgentPlist(text, label);
    assert.equal(parsed.complete, false, `${kind} is not a complete reading`);
    assert.equal(demonstrablyUnregistered(parsed), false, `${kind} must not be accepted as a demonstrated opt-out`);
  }
});

test('non-mac registrations retain the original service-manager semantics', () => {
  assert.equal(demonstrablyUnregistered({ enabled: false }), true);
  assert.equal(demonstrablyUnregistered({ logonTrigger: false }), true);
  assert.equal(demonstrablyUnregistered({ enabled: true }), false);
  assert.equal(demonstrablyUnregistered({}), false, 'an answer with no state is not "disabled"');
});
