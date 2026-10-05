import { readFileSync } from 'node:fs';
const j = JSON.parse(readFileSync(process.argv[2],'utf8').replace(/^\uFEFF/,''));
console.log('session=' + j.session + '  pollMs=' + j.pollMs + '  maxMs=' + j.maxMs + '  serverUp=' + j.serverUp);
console.log('');
console.log('delay  launchToAttach  firstTotal  firstNamed  domAt(launch)  domLatency(attach)  appAlive  polls');
for (const r of j.results) {
  console.log(String(r.firstAttachDelayMs).padStart(5) + '  '
    + String(r.launchToAttachMs).padStart(12) + '  '
    + String(r.firstAttachTotal).padStart(10) + '  '
    + String(r.firstAttachNamed).padStart(10) + '  '
    + String(r.domAtMsFromLaunch === null ? 'NEVER' : r.domAtMsFromLaunch).padStart(13) + '  '
    + String(r.domLatencyFromAttachMs === null ? 'NEVER' : r.domLatencyFromAttachMs).padStart(17) + '  '
    + String(r.appAliveAtEnd).padStart(8) + '  '
    + String(r.pollCount).padStart(5));
}
console.log('');
console.log('=== poll traces ===');
for (const r of j.results) {
  console.log('delay ' + r.firstAttachDelayMs + 'ms: ' + r.polls.map(p => p.atMs + 'ms:' + p.total + '/' + p.named + (p.hasDocument ? ' DOM' : '') + (p.hasNewTerminal ? ' NEWTERM' : '')).join('  '));
}
