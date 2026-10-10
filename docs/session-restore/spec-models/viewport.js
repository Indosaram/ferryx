function rng(seed){ let x = seed>>>0 || 1; return () => { x ^= x<<13; x>>>=0; x ^= x>>17; x ^= x<<5; x>>>=0; return x/4294967296; }; }

export function viewportModel(seed, opts = {}){
  const R = rng(seed);
  const dev = { state: 'Ready', gen: 1, panes: new Set(), reconTok: null, fails: 0 };
  let devQ = [];
  const panes = new Map();
  let work = [];
  const bad = [];
  let shown = new Map();
  function mkPane(id){
    const p = { id, latest: null, committed: null, lastVisible: null, attempt: 0, gen: 0, gpu: null, registered: false, deferred: true, tomb: false, receipts: [], configureCalls: 0 };
    panes.set(id, p); devQ.push({ k: 'Register', pane: id });
    return p;
  }
  const hidden = (t) => !t.visible || t.w === 0 || t.h === 0;
  function prepare(p){
    if (p.tomb || !p.latest) return;
    if (hidden(p.latest)) { work.push({ pane: p.id, cand: { kind: 'Hidden', target: p.latest, attempt: p.attempt } }); return; }
    if (!p.registered || p.gpu !== 'Ready') { p.deferred = true; return; }
    p.deferred = false;
    work.push({ pane: p.id, cand: { kind: 'Visible', target: p.latest, attempt: p.attempt, gen: p.gen } });
  }
  function retryLatest(p){ if (p.tomb) return; p.attempt++; prepare(p); }
  function accept(p, t){
    if (p.tomb) return;
    if (p.latest && (t.bg < p.latest.bg || (t.bg === p.latest.bg && t.rev <= p.latest.rev))) return;
    p.latest = t; p.attempt++; prepare(p);
  }
  function apply(p, c){
    if (p.tomb) { if (c.kind) shown.delete(p.id); return; }
    if (c.attempt !== p.attempt) return;
    if (c.kind === 'Visible' && c.gen !== p.gen) return;
    if (c.kind === 'Hidden'){ shown.set(p.id, { visible: false, target: c.target }); p.committed = c.target; p.receipts.push('Hidden'); return; }
    if (p.gpu !== 'Ready') { p.receipts.push('Degraded'); retryLatest(p); return; }
    if (R() < 0.1){ p.receipts.push('Degraded'); work.push({ pane: p.id, retry: true }); return; }
    p.configureCalls++;
    shown.set(p.id, { visible: true, target: c.target, gen: p.gen });
    p.committed = c.target; p.lastVisible = c.target; p.receipts.push('Ready');
  }
  function devStep(){
    if (!devQ.length) return;
    const m = devQ.splice(Math.floor(R() * Math.min(devQ.length, 2)), 1)[0];
    if (m.k === 'Register'){ dev.panes.add(m.pane); paneMsg(m.pane, { k: 'Registered', state: dev.state, gen: dev.gen }); }
    else if (m.k === 'Unregister'){ dev.panes.delete(m.pane); }
    else if (m.k === 'Lost'){ dev.state = 'Lost'; dev.gen++; dev.fails = 0; for (const id of dev.panes) paneMsg(id, { k: 'DeviceLost', gen: dev.gen }); startRecon(); }
    else if (m.k === 'ReconResult'){
      if (m.tok !== dev.gen || dev.state === 'Ready') return;
      if (m.ok){ dev.state = 'Ready'; for (const id of dev.panes) paneMsg(id, { k: 'DeviceReady', gen: dev.gen }); }
      else { dev.fails++; if (dev.fails >= 3) for (const id of dev.panes) paneMsg(id, { k: 'DeviceDegraded', gen: dev.gen }); startRecon(); }
    }
  }
  let reconWork = [];
  let failMode = false;
  function startRecon(){ reconWork.push({ tok: dev.gen }); }
  const paneInbox = new Map();
  function paneMsg(id, m){ if (!paneInbox.has(id)) paneInbox.set(id, []); paneInbox.get(id).push(m); }
  function paneDeliver(id){
    const q = paneInbox.get(id); if (!q || !q.length) return;
    const m = q.shift(); const p = panes.get(id); if (!p) return;
    if (p.tomb) return;
    if (m.k === 'Registered'){ p.registered = true; p.gpu = m.state; p.gen = m.gen; if (p.deferred && p.gpu === 'Ready') retryLatest(p); }
    else if (m.k === 'DeviceLost'){ if (m.gen <= p.gen) return; p.gpu = 'Lost'; p.gen = m.gen; p.receipts.push('Degraded'); }
    else if (m.k === 'DeviceReady'){ if (m.gen < p.gen) return; p.gpu = 'Ready'; p.gen = m.gen; retryLatest(p); }
    else if (m.k === 'DeviceDegraded'){ if (m.gen !== p.gen || p.gpu === 'Ready') return; p.gpu = 'Degraded'; p.receipts.push('Degraded'); }
  }
  let ids = 0; let bgCounter = 1;
  const STEPS = 2500;
  for (let i = 0; i < STEPS + 400000; i++){
    const quiet = i >= STEPS;
    if (quiet && !work.length && !devQ.length && !reconWork.length && ![...paneInbox.values()].some(q => q.length)) break;
    if (i === STEPS) failMode = false;
    const r = R();
    const live = [...panes.values()].filter(p => !p.tomb);
    if (!quiet){
      if (r < 0.03 || live.length === 0){ mkPane(++ids); continue; }
      if (r < 0.10){ const p = live[Math.floor(R() * live.length)]; const t = { bg: p.latest ? p.latest.bg + (R() < 0.2 ? 1 : 0) : 1, rev: (p.latest ? p.latest.rev : 0) + 1 + Math.floor(R()*2), visible: R() < 0.7, w: R() < 0.1 ? 0 : 100 + Math.floor(R()*50), h: 80 }; accept(p, t); continue; }
      if (r < 0.11){ const p = live[Math.floor(R() * live.length)]; p.tomb = true; if (opts.destroyHides) shown.delete(p.id); devQ.push({ k: 'Unregister', pane: p.id }); continue; }
      if (r < 0.125){ devQ.push({ k: 'Lost' }); continue; }
      if (r < 0.13){ failMode = !failMode; continue; }
    }
    if (r < 0.35 && work.length){ const w = work.splice(Math.floor(R() * work.length), 1)[0]; const p = panes.get(w.pane); if (w.retry) retryLatest(p); else apply(p, w.cand); continue; }
    if (r < 0.5 && reconWork.length){ const w = reconWork.splice(Math.floor(R() * reconWork.length), 1)[0]; devQ.push({ k: 'ReconResult', tok: w.tok, ok: !failMode && R() < 0.8 }); continue; }
    if (r < 0.65){ devStep(); continue; }
    const pids = [...paneInbox.keys()].filter(k => paneInbox.get(k).length); if (pids.length) paneDeliver(pids[Math.floor(R() * pids.length)]);
  }
  for (const p of panes.values()){
    if (p.tomb){ if (shown.has(p.id) && shown.get(p.id).visible) bad.push(`tomb ${p.id} still shown`); continue; }
    if (!p.latest) continue;
    const s = shown.get(p.id);
    if (hidden(p.latest)){ if (s && s.visible) bad.push(`pane ${p.id} should be hidden`); continue; }
    if (!s || !s.visible || s.target !== p.latest) bad.push(`pane ${p.id} not converged gpu=${p.gpu} reg=${p.registered} def=${p.deferred} dev=${dev.state}@${dev.gen} pgen=${p.gen} inbox=${(paneInbox.get(p.id)||[]).length}`);
    else if (s.gen !== dev.gen) bad.push(`pane ${p.id} shown with stale gen ${s.gen} dev=${dev.gen}`);
  }
  if (dev.state !== 'Ready') bad.push(`device not ready ${dev.state}`);
  return bad.length ? { seed, bad: bad.slice(0, 4) } : null;
}
