export const runLease2 = function(seed, opts={}){
  const R = rng(seed);
  const host = { epoch: 0, holder: null, applied: {epoch:0, seq:0}, revokees: new Set() };
  let net = [];
  const mk = id => ({ id, policy:false, gen:1, lease:null, needs_reclaim:false, pending:null, retry_timer:false, suppressed:false, sup_epoch:0, needs_probe:false, next_seq:1, latest_sent:null, pty_acked:null, reqid:0, conn:0 });
  const panes = { A: mk('A'), B: mk('B'), C: mk('C') };
  const P = Object.values(panes);
  let quiet=false, grantsQuiet=0;
  const send=(p,m)=>net.push({to:'host', from:p.id, conn:p.conn, msg:m});
  const policy=p=>p.policy && !p.suppressed;
  function invalidate(p){ if (p.pending && p.pending.kind==='Reclaim') p.pending=null; p.lease=null; p.needs_reclaim=false; p.latest_sent=null; p.pty_acked=null; }
  function suppress(p, e){ p.suppressed=true; p.sup_epoch=e; }
  function unsuppress(p){ p.suppressed=false; p.needs_probe=false; }
  function relinquish(p){ if (p.lease){ send(p,{t:'Release', epoch:p.lease.epoch, cid:p.id}); invalidate(p);} if (p.pending && p.pending.kind==='Acquire') p.pending=null; }
  function ptysync(p){ if (p.lease && !p.needs_reclaim && policy(p) && p.lease.gen===p.gen && !p.latest_sent){ const seq=p.next_seq++; p.latest_sent={seq}; send(p,{t:'Resize', epoch:p.lease.epoch, seq}); } }
  function reeval(p){
    if (!policy(p) && p.lease) { relinquish(p); return; }
    if (p.lease && p.needs_reclaim && !p.pending && !p.retry_timer){ const id=++p.reqid; p.pending={id,kind:'Reclaim',gen:p.lease.gen,epoch:p.lease.epoch}; send(p,{t:'Reclaim',id,epoch:p.lease.epoch,cid:p.id}); return; }
    if (!p.lease && p.suppressed && p.needs_probe && !p.pending && !p.retry_timer){ const id=++p.reqid; p.pending={id,kind:'Probe',epoch:p.sup_epoch}; send(p,{t:'Reclaim',id,epoch:p.sup_epoch,cid:p.id}); return; }
    if (policy(p) && !p.lease && !p.pending && !p.retry_timer){ const id=++p.reqid; p.pending={id,kind:'Acquire',gen:p.gen}; send(p,{t:'Acquire',id,cid:p.id}); }
  }
  function hostHandle(e){
    const m=e.msg; const reply=(to,r)=>net.push({to, conn:panes[to].conn, msg:r});
    const vacate=()=>{ host.holder=null; if (opts.vacate){ for (const r of host.revokees) reply(r,{t:'Vacated'}); } host.revokees.clear(); };
    if (R()<0.05 && m.t!=='Release'){ reply(m.cid||e.from,{t:'Error',id:m.id}); return; }
    if (m.t==='Acquire'){ const prev=host.holder; host.epoch++; host.holder=m.cid; if(prev && prev!==m.cid){ reply(prev,{t:'Revoked',revoked:host.epoch-1,new:host.epoch}); host.revokees.add(prev);} host.revokees.delete(m.cid); if(quiet) grantsQuiet++; reply(m.cid,{t:'Granted',id:m.id,epoch:host.epoch}); }
    else if (m.t==='Release'){ if (m.epoch===host.epoch && host.holder===m.cid) vacate(); }
    else if (m.t==='Reclaim'){ const ok = m.epoch===host.epoch && host.holder===m.cid; if(!ok && host.holder && host.holder!==m.cid) host.revokees.add(m.cid); reply(m.cid,{t:'ReclaimResult',id:m.id,req:m.epoch,ok,cur: host.holder? host.epoch:0, applied:{...host.applied}}); }
    else if (m.t==='Resize'){ if (m.epoch!==host.epoch || host.holder!==e.from) reply(e.from,{t:'StaleLease',epoch:m.epoch,cur:host.holder?host.epoch:0}); else if (m.epoch===host.applied.epoch && m.seq<=host.applied.seq) reply(e.from,{t:'StaleResize',applied:{...host.applied}}); else { host.applied={epoch:m.epoch,seq:m.seq}; reply(e.from,{t:'ResizeAck',epoch:m.epoch,seq:m.seq}); } }
  }
  function paneHandle(p, m){
    if (m.t==='Granted'){
      if (!p.pending || p.pending.kind!=='Acquire' || p.pending.id!==m.id){ send(p,{t:'Release',epoch:m.epoch,cid:p.id}); }
      else { const c=p.pending; p.pending=null; if (c.gen===p.gen && policy(p)){ p.lease={epoch:m.epoch,gen:p.gen}; p.needs_reclaim=false; unsuppress(p); p.next_seq=1; p.latest_sent=null; p.pty_acked=null; ptysync(p);} else send(p,{t:'Release',epoch:m.epoch,cid:p.id}); }
    } else if (m.t==='Revoked'){ if (p.lease && p.lease.epoch===m.revoked){ invalidate(p); if (m.new) suppress(p,m.revoked); } }
    else if (m.t==='Vacated'){ unsuppress(p); }
    else if (m.t==='Error'){ if (p.pending && p.pending.id===m.id){ p.pending=null; p.retry_timer=true; } }
    else if (m.t==='ReclaimResult'){
      if (p.pending && p.pending.id===m.id){ const c=p.pending; p.pending=null;
        if (c.kind==='Probe'){ p.needs_probe=false; if (!m.cur) unsuppress(p); }
        else if (p.lease && p.lease.epoch===c.epoch && p.lease.gen===c.gen){ if (m.ok){ p.needs_reclaim=false; p.next_seq=Math.max(p.next_seq,m.applied.seq+1); p.latest_sent=null; ptysync(p);} else { invalidate(p); if (m.cur) suppress(p,c.epoch); } } }
    } else if (m.t==='StaleLease'){ if (p.lease && p.lease.epoch===m.epoch){ invalidate(p); if (m.cur) suppress(p,m.epoch); } }
    else if (m.t==='StaleResize'){ if (p.lease && m.applied.epoch===p.lease.epoch){ p.next_seq=Math.max(p.next_seq,m.applied.seq+1); p.latest_sent=null; ptysync(p);} }
    else if (m.t==='ResizeAck'){ if (p.lease && p.lease.epoch===m.epoch && (!p.pty_acked || m.seq>p.pty_acked.seq)) p.pty_acked={seq:m.seq}; }
    reeval(p);
  }
  const steps=500;
  for (let i=0;i<steps+4000;i++){
    const r=R(); const pick=()=>P[Math.floor(R()*P.length)];
    if (i===steps){ quiet=true; }
    if (!quiet && r<0.06){ const p=pick(); p.policy=!p.policy; if(p.policy) unsuppress(p); reeval(p); continue; }
    if (!quiet && r<0.09){ const p=pick(); p.gen++; relinquish(p); reeval(p); continue; }
    if (r<0.12 && (!quiet || i<steps+200)){ const p=pick(); p.conn++; net=net.filter(e=>!(e.to===p.id||e.from===p.id)); p.pending=null; p.retry_timer=false; if(p.lease) p.needs_reclaim=true; if(p.suppressed && opts.probe) p.needs_probe=true; reeval(p); continue; }
    if (r<0.18){ for (const p of P) if (p.retry_timer){ p.retry_timer=false; reeval(p);} continue; }
    if (r<0.25){ for (const p of P) if (p.lease && p.latest_sent && (!p.pty_acked || p.pty_acked.seq<p.latest_sent.seq) && !net.some(e=>(e.msg.t==='Resize'&&e.from===p.id)||(e.msg.t==='ResizeAck'&&e.to===p.id)||(e.msg.t==='StaleLease'&&e.to===p.id))){ p.latest_sent=null; ptysync(p); reeval(p);} continue; }
    if (net.length===0) continue;
    const e=net.splice(Math.floor(R()*net.length),1)[0];
    if (e.to==='host'){ if (e.conn!==panes[e.from].conn) {/* in-flight on old conn already filtered */} hostHandle(e); } else { if (e.conn!==panes[e.to].conn) continue; paneHandle(panes[e.to], e.msg); }
  }
  // Liveness: each pane with policy (raw) true & not suppressed-by-live-holder should... Check: holder consistency + no stuck
  const bad=[];
  for (const p of P){
    if (p.lease && !(host.holder===p.id && host.epoch===p.lease.epoch)) bad.push(`${p.id} thinks lease ${p.lease.epoch} host ${host.holder}@${host.epoch}`);
    if (p.policy && !p.suppressed && !p.lease) bad.push(`${p.id} eligible but no lease`);
    if (p.suppressed && host.holder===null && p.policy) bad.push(`${p.id} suppressed while vacant`);
    if (p.lease && p.latest_sent && host.applied.epoch!==p.lease.epoch) bad.push(`${p.id} resize not applied`);
  }
  if (!host.holder && P.some(p=>p.policy)) bad.push('vacant with eligible pane');
  return bad.length? {seed, bad, host:{epoch:host.epoch,holder:host.holder}, grantsQuiet} : (grantsQuiet>1? {seed, bad:['churn in quiet: '+grantsQuiet]} : null);
};
