// One-shot codegen: enumerate owned windows with the driver's OWN enumeration,
// then write the driver's OWN click script to a file and exit. No node in the
// measurement path afterwards.
import { writeFileSync } from 'node:fs';
import { awaitOwnedWindowsWindows, buildWindowsNewPaneScript } from '../lib/qa-scenarios/native-driver.mjs';

const pid = Number(process.argv[2]);
const clickOut = process.argv[3];
const infoOut = process.argv[4];
const info = { pid, ok: false };
try {
  const enumeration = await awaitOwnedWindowsWindows({ action: () => {} }, pid);
  info.visibleWindowCount = enumeration.visibleWindowCount;
  info.searchOrder = enumeration.searchOrder;
  const script = buildWindowsNewPaneScript(pid, { windows: enumeration.searchOrder });
  writeFileSync(clickOut, script);
  info.clickScriptBytes = script.length;
  info.ok = true;
} catch (error) {
  info.error = String(error && error.message ? error.message : error);
}
writeFileSync(infoOut, JSON.stringify(info, null, 2));
process.exit(0);
