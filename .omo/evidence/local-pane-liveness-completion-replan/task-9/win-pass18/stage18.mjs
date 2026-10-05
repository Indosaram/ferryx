import { readFileSync, writeFileSync, existsSync } from 'node:fs';

const ROOT = '/tmp/t9p10/stage18';
const problems = [];
const applied = [];

function patch(rel, label, from, to, expectCount = 1) {
  const p = ROOT + '/' + rel;
  const src = readFileSync(p, 'utf8');
  const hits = src.split(from).length - 1;
  if (hits !== expectCount) { problems.push(`${label}: anchor found ${hits}x, expected ${expectCount}`); return; }
  const out = src.split(from).join(to);
  writeFileSync(p, out, 'utf8');
  // VERIFY BY READING BACK THE PATCHED TEXT, not by "the file changed".
  const back = readFileSync(p, 'utf8');
  const needle = to.includes('\n') ? to.split('\n').filter(l => l.trim()).pop() : to;
  if (!back.includes(needle.trim())) { problems.push(`${label}: patched text NOT present after write`); return; }
  applied.push(`${label} (anchor ${hits}x, verified present)`);
}

// ---- A: pane-liveness.mjs — the probe import + two call sites ----
const PL = 'scripts/qa/pane-liveness.mjs';
const IMPORT_OLD = "import { bindPaneSession, createPaneInventoryReader } from '../lib/qa-scenarios/pane-binding.mjs';";
patch(PL, 'A1 probe import',
  IMPORT_OLD,
  IMPORT_OLD + "\n// VERIFIER PROBE (pass 18, staged copy only): records the app's UIA tree, the isolated\n// daemon inventory and the barrier receipts at the pane step, stamped with the\n// interpreter + version that produced them.\nimport { probePane } from '../lib/qa-scenarios/verifier-probe.mjs';");
patch(PL, 'A2 after-newPane call',
  '    await driver.newPane(evidence, pid);\n    paneBinding = await bindPaneSession({',
  "    await driver.newPane(evidence, pid);\n    probePane({ pid, isolationRoot: context.isolationRoot, evidenceDir: context.evidenceDir, label: 'after-newPane' });\n    paneBinding = await bindPaneSession({");
patch(PL, 'A3 after-bind call',
  '    ctx.paneBinding = paneBinding;',
  "    ctx.paneBinding = paneBinding;\n    probePane({ pid, isolationRoot: context.isolationRoot, evidenceDir: context.evidenceDir, label: 'after-bind', session: paneBinding?.backendSessionId ?? null });");

// ---- B: windows-interactive.mjs — first-line marker + marker path outliving the bat dir ----
const WI = 'scripts/lib/qa-scenarios/windows-interactive.mjs';
const BAT_OLD = [
  "  const batBody = [",
  "    '@echo off',",
  "    `set ${WINDOWS_INTERACTIVE_RELAUNCH_ENV}=1`,",
  "    `set ${WINDOWS_RELAUNCH_RECORD_ENV}=${recordPath}`,",
  "    `cd /d ${quoteBatArg(cwd)}`,",
  "    `${command} > ${quoteBatArg(outPath)} 2> ${quoteBatArg(errPath)}`,",
  "    `echo %ERRORLEVEL% > ${quoteBatArg(exitPath)}`,",
  "  ].join('\\r\\n');",
].join('\n');
const BAT_NEW = [
  "  // VERIFIER INSTRUMENTATION (pass 18, staged copy only). The FIRST line writes a",
  "  // marker: pass 17 measured that a stalled attempt never executes line 1 at all,",
  "  // so this marker is the only thing that can distinguish a stall from a slow run.",
  "  const markerPath = join(markerDir, 'relaunch.markers.log');",
  "  const M = () => quoteBatArg(markerPath);",
  "  const batBody = [",
  "    '@echo off',",
  "    'setlocal enabledelayedexpansion',",
  "    `echo [%DATE% %TIME%] 00_FIRST_LINE >> ${M()}`,",
  "    `set ${WINDOWS_INTERACTIVE_RELAUNCH_ENV}=1`,",
  "    `set ${WINDOWS_RELAUNCH_RECORD_ENV}=${recordPath}`,",
  "    `echo [%DATE% %TIME%] 01_ENTRY cwd=%CD% >> ${M()}`,",
  "    `cd /d ${quoteBatArg(cwd)}`,",
  "    `echo [%DATE% %TIME%] 02_AFTER_CD cwd=%CD% err=!ERRORLEVEL! >> ${M()}`,",
  "    `where node >> ${M()} 2>&1`,",
  "    `echo [%DATE% %TIME%] 04_AFTER_WHERE_NODE err=!ERRORLEVEL! >> ${M()}`,",
  "    `echo [%DATE% %TIME%] 05_BEFORE_NODE >> ${M()}`,",
  "    `${command} > ${quoteBatArg(outPath)} 2> ${quoteBatArg(errPath)}`,",
  "    `echo [%DATE% %TIME%] 06_AFTER_NODE err=!ERRORLEVEL! >> ${M()}`,",
  "    `echo !ERRORLEVEL! > ${quoteBatArg(exitPath)}`,",
  "    `echo [%DATE% %TIME%] 07_EXIT >> ${M()}`,",
  "  ].join('\\r\\n');",
].join('\n');
patch(WI, 'B1 bat first-line marker + steps', BAT_OLD, BAT_NEW);
patch(WI, 'B2 markerPath in plan', '    taskName, batPath, batBody, outPath, errPath, exitPath, recordPath, command, timeoutMs,', '    taskName, batPath, batBody, outPath, errPath, exitPath, recordPath, markerPath, command, timeoutMs,');
patch(WI, 'B3 markerDir param', '  taskName, batDir, nodePath, runnerPath, runnerArgs, cwd, timeoutMs = BUDGETS.interactiveRelaunchTimeoutMs,\n}) {', '  taskName, batDir, nodePath, runnerPath, runnerArgs, cwd, timeoutMs = BUDGETS.interactiveRelaunchTimeoutMs,\n  // VERIFIER (pass 18): the marker log must outlive the bat dir, which is cleaned up.\n  markerDir = batDir,\n}) {');
patch(WI, 'B4 pass markerDir', '    nodePath: context.argv[0], runnerPath: context.argv[1], runnerArgs: rawArgv, cwd,\n  });', '    nodePath: context.argv[0], runnerPath: context.argv[1], runnerArgs: rawArgv, cwd,\n    markerDir: context.evidenceDir,\n  });');
patch(WI, 'B5 record markerPath', '    batPath: plan.batPath,\n    evidenceDir: context.evidenceDir,\n  };', '    batPath: plan.batPath,\n    markerPath: plan.markerPath,\n    evidenceDir: context.evidenceDir,\n  };');

console.log('APPLIED:');
for (const a of applied) console.log('  OK ' + a);
console.log('PROBLEMS: ' + problems.length);
for (const p of problems) console.log('  !! ' + p);
if (problems.length) process.exit(1);
