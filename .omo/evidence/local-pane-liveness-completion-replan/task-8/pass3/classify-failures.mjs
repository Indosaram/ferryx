// Classify pass-3 full-UI failures against the pass-2 A/B classifications.
// Reads the host full-ui logs, extracts failed test files, and joins them with pass2/baseline-classification.md.
import fs from 'node:fs';
import path from 'node:path';
const here = path.dirname(new URL(import.meta.url).pathname);
const p2 = path.join(here, '..', 'pass2');
const pass2Lists = {
  linux: JSON.parse(fs.readFileSync(path.join(p2, 'linux-ui-failed-files.json'), 'utf8')),
  windows: JSON.parse(fs.readFileSync(path.join(p2, 'windows-ui-failed-files.json'), 'utf8')),
  mac: (JSON.parse(fs.readFileSync(path.join(p2, 'mac-ui-failed-files.json'), 'utf8')).files) || [],
};
const protectedFiles = ['src/components/TerminalSearchOverlay.test.tsx', 'src/lib/updater.test.ts'];
const mandatory = ['src/components/TerminalSplitView.paneHandleReach.test.tsx', 'src/lib/pairedDaemonRollout.test.ts'];
const out = {};
for (const host of ['mac', 'linux', 'windows']) {
  const logPath = path.join(here, host, 'logs', 'full-ui.log');
  if (!fs.existsSync(logPath)) { out[host] = { status: 'NO_LOG' }; continue; }
  const text = fs.readFileSync(logPath, 'utf8').replace(/\x1b\[[0-9;]*m/g, '');
  const files = new Set();
  for (const m of text.matchAll(/^\s*(?:FAIL|\u00d7)\s+(\S+\.(?:test|spec)\.(?:tsx?|mts))/gm)) files.add(m[1]);
  const summary = text.match(/Tests\s+(?:(\d+) failed \| )?(\d+) passed \((\d+)\)/);
  const filesSummary = text.match(/Test Files\s+(?:(\d+) failed \| )?(\d+) passed \((\d+)\)/);
  const timedOut = fs.existsSync(path.join(here, host, 'timeout-full-ui.json'));
  const failed = [...files].sort();
  const p2set = new Set(pass2Lists[host] || []);
  out[host] = {
    timedOut,
    tests: summary ? { failed: Number(summary[1] || 0), passed: Number(summary[2]), total: Number(summary[3]) } : null,
    testFiles: filesSummary ? { failed: Number(filesSummary[1] || 0), passed: Number(filesSummary[2]), total: Number(filesSummary[3]) } : null,
    failedFileCount: failed.length,
    knownFromPass2: failed.filter(f => p2set.has(f)),
    NEW_IN_PASS3: failed.filter(f => !p2set.has(f)),
    pass2HadButPass3DoesNot: [...p2set].filter(f => !failed.includes(f)),
    protectedStillFailing: failed.filter(f => protectedFiles.includes(f)),
    mandatoryStillFailing: failed.filter(f => mandatory.includes(f)),
  };
}
console.log(JSON.stringify(out, null, 2));
