import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
const root = process.argv[2];
function walk(d, out=[]) { for (const e of readdirSync(d)) { const p = join(d,e); if (statSync(p).isDirectory()) walk(p,out); else if (e === 'actions.jsonl') out.push(p); } return out; }
for (const f of walk(root).sort()) {
  const scenario = f.split('/runs/')[1].split('/')[0];
  const objs = readFileSync(f,'utf8').trim().split('\n').map(l => JSON.parse(l));
  console.log('########## ' + scenario);
  console.log('  order: ' + objs.map(o => o.action).join(' | '));
  for (const act of ['click-pane-affordance','pane-session-bound','click-split-affordance']) {
    const o = objs.filter(x => x.action === act).pop();
    if (!o) { console.log('  ' + act + ': ABSENT'); continue; }
    console.log('  === ' + act + ' ===');
    console.log('    ' + JSON.stringify(o));
  }
}
