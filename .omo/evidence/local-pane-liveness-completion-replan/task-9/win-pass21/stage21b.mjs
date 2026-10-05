import { readFileSync, writeFileSync } from 'node:fs';
const ROOT = '/tmp/t9p10/stage21b';
const problems = [], applied = [];
function patch(rel, label, from, to) {
  const p = ROOT + '/' + rel;
  const src = readFileSync(p, 'utf8');
  const hits = src.split(from).length - 1;
  if (hits !== 1) { problems.push(label + ': anchor ' + hits + 'x, expected 1'); return; }
  writeFileSync(p, src.split(from).join(to), 'utf8');
  const back = readFileSync(p, 'utf8');
  const needle = to.split('\n').filter(l => l.trim()).pop().trim();
  if (!back.includes(needle)) { problems.push(label + ': not present after write'); return; }
  applied.push(label + ' (anchor 1x, verified)');
}
const PL = 'scripts/qa/pane-liveness.mjs';
const IMP = "import { bindPaneSession, createPaneInventoryReader } from '../lib/qa-scenarios/pane-binding.mjs';";
patch(PL, 'probe import', IMP, IMP + "\n// VERIFIER PROBE (pass 21, staged copy only, from the committed blob).\nimport { probePane } from '../lib/qa-scenarios/verifier-probe.mjs';");
patch(PL, 'after-newPane', '    await driver.newPane(evidence, pid);\n    paneBinding = await bindPaneSession({',
  "    await driver.newPane(evidence, pid);\n    probePane({ pid, isolationRoot: ctx.isolationRoot, evidenceDir: evidence.runDir, label: 'after-newPane' });\n    paneBinding = await bindPaneSession({");
patch(PL, 'after-bind', '    ctx.paneBinding = paneBinding;',
  "    ctx.paneBinding = paneBinding;\n    probePane({ pid, isolationRoot: ctx.isolationRoot, evidenceDir: evidence.runDir, label: 'after-bind', session: paneBinding?.backendSessionId ?? null });");
console.log('APPLIED:'); for (const a of applied) console.log('  OK ' + a);
console.log('PROBLEMS: ' + problems.length); for (const p of problems) console.log('  !! ' + p);
if (problems.length) process.exit(1);
