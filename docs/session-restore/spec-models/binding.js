function rng(seed){ let x = seed>>>0 || 1; return () => { x ^= x<<13; x>>>=0; x ^= x>>17; x ^= x<<5; x>>>=0; return x/4294967296; }; }

export function bindingModel(seed, opts = {}){
  const R = rng(seed);
  const host = { alive: true, reachable: true, tableRev: 1, session: { id: 'S', inc: 'I1', running: true, createdRev: 1, deletedAt: null, exitedAt: null }, subs: new Map(), nextSub: 1, conn: 0 };
  const truth = () => host.alive ? (host.session.deletedAt !== null ? 'absent' : (host.session.running ? 'alive' : 'exited')) : 'dead';
  let net = [];
  let invSeq = 0;
  const B = { st: 'resolving', inc: opts.adopt ? null : 'I1', partition: opts.adopt ? null : 'P', createdRev: opts.adopt ? null : 1, attachSeq: 0, subId: null, lastToken: 0, conn: 0, ackTimer: false, backoff: false, everTerminated: null };
  const subscriberId = 'pane-1';
  const sendClient = (m) => net.push({ to: 'host', conn: B.conn, m });
  const toClient = (m) => net.push({ to: 'client', conn: host.conn, m });
  const bad = [];
  function terminate(kind){
    const t = truth();
    if (kind === 'absent' && t !== 'absent') bad.push(`terminated(absent) while ${t}`);
    if (kind === 'exited' && !(t === 'exited' || t === 'absent' || (!host.alive && host.session.exitedAt !== null))) bad.push(`terminated(exited) while ${t}`);
    if (kind === 'owner_lost' && t !== 'dead') bad.push(`terminated(owner_lost) while ${t}`);
    B.st = 'terminated'; B.everTerminated = kind; B.ackTimer = false;
  }
  function requestInventory(){ invSeq++; net.push({ to: 'daemon-inv', conn: B.conn, m: { token: invSeq } }); }
  function enterAttaching(){ B.st = 'attaching'; B.attachSeq++; B.ackTimer = true; sendClient({ t: 'Subscribe', sub: subscriberId, seq: B.attachSeq }); }
  function hostInventory(token){
    if (!host.alive) return { token, reach: false, complete: false, ownerDead: opts.deadInInventory !== false };
    if (!host.reachable) return { token, reach: false, complete: false };
    const s = host.session;
    const entries = s.deletedAt === null ? [{ id: s.id, inc: s.inc, running: s.running, createdRev: s.createdRev }] : [];
    return { token, reach: true, complete: true, tableRev: host.tableRev, entries };
  }
  function onInventory(inv){
    if (B.st !== 'resolving') return;
    if (inv.token < B.lastToken) return;
    B.lastToken = inv.token;
    if (inv.ownerDead) { terminate('owner_lost'); return; }
    if (!inv.reach || !inv.complete) { B.backoff = true; return; }
    let e = inv.entries.find(x => x.id === 'S');
    if (B.adoptPending || opts.adopt && B.inc === null) {
      if (e) { B.inc = e.inc; B.createdRev = e.createdRev; B.partition = 'P'; }
      else { terminate('absent'); return; }
    }
    if (e && e.inc !== B.inc) e = null;
    if (e && e.running) { enterAttaching(); return; }
    if (e && !e.running) { terminate('exited'); return; }
    if (B.createdRev !== null && inv.tableRev >= B.createdRev) { terminate('absent'); return; }
    B.backoff = true;
  }
  function hostHandle(ev){
    if (!host.alive || !host.reachable || ev.conn !== host.conn) return;
    const m = ev.m;
    if (m.t === 'Subscribe'){
      if (host.session.deletedAt !== null) { toClient({ t: 'NotFound' }); return; }
      const cur = host.subs.get(m.sub);
      if (cur && cur.seq >= m.seq) { toClient({ t: 'StaleSubscribe', current: cur.seq }); return; }
      if (cur) host.subs.delete(m.sub);
      if (opts.subLimit && host.subs.size >= opts.subLimit) { toClient({ t: 'Limit' }); return; }
      const id = host.nextSub++;
      host.subs.set(m.sub, { seq: m.seq, id, conn: ev.conn });
      toClient({ t: 'SubscribeAck', seq: m.seq, id, inc: host.session.inc, exited: !host.session.running });
    }
  }
  function clientHandle(ev){
    if (ev.conn !== B.conn) return;
    const m = ev.m;
    if (B.st === 'terminated') return;
    if (m.t === 'SubscribeAck'){
      if (m.seq !== B.attachSeq || B.st !== 'attaching') return;
      if (m.inc !== B.inc) return;
      B.subId = m.id; B.st = 'attached'; B.ackTimer = false;
      if (m.exited) terminate('exited');
    } else if (m.t === 'NotFound'){ if (B.st === 'attaching'){ B.st = 'resolving'; B.ackTimer = false; requestInventory(); } }
    else if (m.t === 'StaleSubscribe'){ if (B.st === 'attaching'){ B.attachSeq = m.current; enterAttaching(); } }
    else if (m.t === 'Limit'){ if (B.st === 'attaching'){ B.st = 'disconnected'; B.ackTimer = false; B.backoff = true; } }
    else if (m.t === 'ChildExited'){ if (m.inc === B.inc && B.st !== 'unbound') terminate('exited'); }
    else if (m.t === 'OwnerDead'){ terminate('owner_lost'); }
  }
  function connDrop(){
    host.conn++; B.conn = host.conn;
    net = net.filter(e => e.to === 'daemon-inv' ? R() < 0.5 : false);
    for (const [k, v] of host.subs) host.subs.delete(k);
    if (B.st === 'attaching' || B.st === 'attached'){ B.st = 'disconnected'; B.ackTimer = false; B.backoff = true; }
  }
  function childExit(){
    if (!host.alive || !host.session.running || host.session.deletedAt !== null) return;
    host.session.running = false; host.session.exitedAt = 1; host.tableRev++;
    if (host.reachable) toClient({ t: 'ChildExited', inc: host.session.inc });
  }
  function deleteSession(){
    if (!host.alive || host.session.running || host.session.deletedAt !== null || host.subs.size) return;
    host.session.deletedAt = 1; host.tableRev++;
  }
  function hostDie(){ if (!host.alive) return; host.alive = false; net.push({ to: 'client-ownerdead', conn: B.conn, m: { t: 'OwnerDead' } }); }
  requestInventory();
  const STEPS = 3000;
  for (let i = 0; i < STEPS + 6000; i++){
    const quiet = i >= STEPS;
    const r = R();
    if (!quiet){
      if (r < 0.03){ connDrop(); continue; }
      if (r < 0.06){ host.reachable = !host.reachable; if (!host.reachable) connDrop(); continue; }
      if (r < 0.065 && opts.exit){ childExit(); continue; }
      if (r < 0.068 && opts.exit){ deleteSession(); continue; }
      if (r < 0.069 && opts.die){ hostDie(); continue; }
    } else if (i === STEPS) { host.reachable = true; }
    if (r < 0.15){
      if (B.ackTimer && B.st === 'attaching' && !net.some(e => (e.to === 'host' && e.m.t === 'Subscribe') || (e.to === 'client' && ['SubscribeAck','StaleSubscribe','NotFound','Limit'].includes(e.m.t)))){ B.st = 'disconnected'; B.ackTimer = false; B.backoff = true; }
      if (B.backoff){ B.backoff = false; if (B.st === 'disconnected') B.st = 'resolving'; if (B.st === 'resolving') requestInventory(); }
      else if (opts.resolveTimer !== false && B.st === 'resolving' && !net.some(e => e.to === 'daemon-inv')) requestInventory();
      else if (opts.resolveTimer !== false && B.st === 'disconnected') { B.st = 'resolving'; requestInventory(); }
      continue;
    }
    if (!net.length) continue;
    const ev = net.splice(Math.floor(R() * net.length), 1)[0];
    if (ev.to === 'host') hostHandle(ev);
    else if (ev.to === 'client') clientHandle(ev);
    else if (ev.to === 'client-ownerdead') clientHandle({ ...ev, conn: B.conn });
    else if (ev.to === 'daemon-inv') onInventory(hostInventory(ev.m.token));
  }
  const t = truth();
  if (t === 'alive' && B.st !== 'attached') bad.push(`alive but ${B.st}`);
  if (t === 'exited' && B.everTerminated !== 'exited') bad.push(`exited but ${B.st}/${B.everTerminated}`);
  if (t === 'dead' && B.st !== 'terminated') bad.push(`dead but ${B.st}`);
  if (t === 'absent' && B.st !== 'terminated') bad.push(`absent but ${B.st}`);
  if (host.alive && host.subs.size > 1) bad.push(`subs ${host.subs.size}`);
  return bad.length ? { seed, bad: [...new Set(bad)].slice(0, 4), truth: t, st: B.st } : null;
}
