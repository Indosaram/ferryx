// Executable model of spec §3.3-§3.6 (input lease, ledger, author state machine) + §4.4 shared lease rules.
function rng(seed){ let x = seed>>>0 || 1; return () => { x ^= x<<13; x>>>=0; x ^= x>>17; x ^= x<<5; x>>>=0; return x/4294967296; }; }

export function inputModel(seed, opts = {}){
  const R=rng(seed);
  const H={epoch:0, holder:null, led:new Map(), wq:[], tap:[], revokees:new Set()};
  let net=[];
  const mkA=id=>({id, focus:false, cur:null, needs_reclaim:false, pending:null, retry:false, sup:false, supE:0, probe:false, unassigned:[], seg:new Map(), fin:new Map(), conn:0, req:0, typed:[], status:new Map(), inflight:0, sendTimer:false});
  const A={X:mkA('X'), Y:mkA('Y')}; const L=Object.values(A);
  const send=(a,m)=>net.push({to:'H',from:a.id,conn:a.conn,m});
  const reply=(to,m)=>net.push({to,conn:A[to].conn,m});
  function fence(e){ const l=H.led.get(e); if(l && !l.fenced){ l.fenced=true; if(H.epoch===e) H.holder=null; } }
  function vacate(){ for(const r of H.revokees) reply(r,{t:'Vacated'}); H.revokees.clear(); }
  function host(e){ const m=e.m; const a=e.from;
    if (opts.errRate && R()<opts.errRate && m.t!=='Release' && m.t!=='Write'){ reply(a,{t:'Err',id:m.id}); return; }
    if(m.t==='Acquire'){ const prev=H.holder; if(H.epoch) fence(H.epoch); H.epoch++; H.holder=a; H.led.set(H.epoch,{holder:a,accepted:0,committed:0,dropped:0,fenced:false,bytes:[]}); if(prev&&prev!==a){ reply(prev,{t:'Revoked',e:H.epoch-1,neu:H.epoch}); H.revokees.add(prev);} H.revokees.delete(a); reply(a,{t:'Granted',id:m.id,e:H.epoch}); }
    else if(m.t==='Release'){ const l=H.led.get(m.e); if(l&&!l.fenced&&H.epoch===m.e&&H.holder===a){ fence(m.e); vacate(); } }
    else if(m.t==='Reclaim'){ const l=H.led.get(m.e); if(!l){ reply(a,{t:'Err',id:m.id}); return;} const ok=H.epoch===m.e&&H.holder===a&&!l.fenced; if(!ok&&H.holder&&H.holder!==a) H.revokees.add(a); reply(a,{t:'RR',id:m.id,req:m.e,ok,cur:H.holder?H.epoch:0,acc:l.accepted,c:l.committed,fin:{fa:l.accepted,c:l.committed,d:l.dropped}}); }
    else if(m.t==='Get'){ const l=H.led.get(m.e); reply(a,{t:'ES',id:m.id,e:m.e,fa:l.accepted,c:l.committed,d:l.dropped,fenced:l.fenced}); }
    else if(m.t==='Write'){ const l=H.led.get(m.e); if(!(H.epoch===m.e&&H.holder===a&&!l.fenced)){ reply(a,{t:'Stale',id:m.id,e:m.e,cur:H.holder?H.epoch:0}); return;}
      if(m.start>l.accepted){ reply(a,{t:'Gap',id:m.id,e:m.e,acc:l.accepted}); return; }
      for(let i=m.start;i<Math.min(m.start+m.bytes.length,l.accepted);i++) if(l.bytes[i]!==m.bytes[i-m.start]){ fence(m.e); reply(a,{t:'Revoked',e:m.e,neu:0}); vacate(); return; }
      if(m.start+m.bytes.length>l.accepted && H.wq.length>40){ reply(a,{t:'BP',id:m.id,e:m.e}); return; }
      for(let i=l.accepted-m.start;i<m.bytes.length;i++){ l.bytes.push(m.bytes[i]); H.wq.push({e:m.e,b:m.bytes[i]}); }
      l.accepted=Math.max(l.accepted,m.start+m.bytes.length); reply(a,{t:'Ack',id:m.id,e:m.e,acc:l.accepted,c:l.committed}); }
  }
  function writeOne(){ if(!H.wq.length) return; const w=H.wq.shift(); H.tap.push(w.b); const l=H.led.get(w.e); l.committed++; reply(l.holder,{t:'Ack',id:0,e:w.e,acc:l.accepted,c:l.committed}); }

  // ---- author (§3.6) ----
  const acquireCond=a=>a.focus&&!a.sup&&a.unassigned.length>0;
  function unsent(a){ if(!a.cur) return 0; const s=a.seg.get(a.cur.e); return s.base+s.bytes.length-a.cur.acc; }
  function sendWin(a){ if(!a.cur||a.needs_reclaim||a.sendTimer) return; const s=a.seg.get(a.cur.e);
    while(a.inflight<2){ const from=a.cur.acc + a.inflight*4; const bytes=s.bytes.slice(from-s.base, from-s.base+4); if(!bytes.length) break; a.inflight++; send(a,{t:'Write',id:++a.req,e:a.cur.e,start:from,bytes}); } }
  function lose(a,e,fin,other){ const s=a.seg.get(e); a.seg.delete(e); a.fin.set(e,{s,pend:null,retry:false}); a.cur=null; a.needs_reclaim=false; a.inflight=0; a.sendTimer=false; if(a.pending&&a.pending.k==='Reclaim') a.pending=null; if(other){a.sup=true;a.supE=e;} if(fin) classify(a,e,fin); }
  function classify(a,e,f){ const x=a.fin.get(e); if(!x) return; for(let i=0;i<x.s.bytes.length;i++){ const off=x.s.base+i; const b=x.s.bytes[i]; a.status.set(b, off<f.c?'W':off<f.fa-f.d?'Q':off<f.fa?'D':'N'); } a.fin.delete(e); }
  function reeval(a){
    for(const [e,x] of a.fin) if(!x.pend&&!x.retry){ x.pend=++a.req; send(a,{t:'Get',id:x.pend,e}); }
    if(a.cur && !a.focus && unsent(a)===0 && a.inflight===0){ send(a,{t:'Release',e:a.cur.e}); lose(a,a.cur.e,null,false); }
    else if(a.cur&&a.needs_reclaim&&!a.pending&&!a.retry){ a.pending={k:'Reclaim',id:++a.req,e:a.cur.e}; send(a,{t:'Reclaim',id:a.pending.id,e:a.cur.e}); }
    else if(!a.cur&&a.sup&&a.probe&&!a.pending&&!a.retry){ a.pending={k:'Probe',id:++a.req}; send(a,{t:'Reclaim',id:a.pending.id,e:a.supE}); }
    else if(!a.cur&&acquireCond(a)&&!a.pending&&!a.retry){ a.pending={k:'Acq',id:++a.req}; send(a,{t:'Acquire',id:a.pending.id}); }
    sendWin(a);
  }
  function ack(a,e,acc,c){ if(a.cur&&a.cur.e===e){ a.cur.acc=Math.max(a.cur.acc,acc); a.cur.c=Math.max(a.cur.c,c);} }
  function author(a,m){
    if(m.t==='Granted'){ if(!a.pending||a.pending.k!=='Acq'||a.pending.id!==m.id){ send(a,{t:'Release',e:m.e}); } else { a.pending=null; if(a.focus&&!a.sup){ a.cur={e:m.e,acc:0,c:0}; a.sup=false;a.probe=false; a.seg.set(m.e,{base:0,bytes:a.unassigned}); a.unassigned=[]; a.inflight=0;} else send(a,{t:'Release',e:m.e}); } }
    else if(m.t==='Revoked'){ if(a.cur&&a.cur.e===m.e) lose(a,m.e,null,m.neu!==0); }
    else if(m.t==='Vacated'){ a.sup=false;a.probe=false; }
    else if(m.t==='Ack'){ ack(a,m.e,m.acc,m.c); if(m.id && a.cur&&a.cur.e===m.e) { a.inflight=Math.max(0,a.inflight-1); } }
    else if(m.t==='Stale'){ if(a.cur&&a.cur.e===m.e) lose(a,m.e,null,m.cur!==0); }
    else if(m.t==='Gap'||m.t==='BP'){ if(a.cur&&a.cur.e===m.e){ if(m.acc!==undefined) a.cur.acc=Math.max(a.cur.acc,m.acc); a.inflight=0; a.sendTimer=true; } }
    else if(m.t==='ES'){ const x=a.fin.get(m.e); if(x&&x.pend===m.id){ x.pend=null; if(!m.fenced){ send(a,{t:'Release',e:m.e}); x.retry=true; } else classify(a,m.e,{fa:m.fa,c:m.c,d:m.d}); } }
    else if(m.t==='RR'){ if(a.pending&&a.pending.id===m.id){ const p=a.pending; a.pending=null; if(p.k==='Probe'){ a.probe=false; if(!m.cur) a.sup=false; } else if(a.cur&&a.cur.e===p.e){ if(m.ok){ a.needs_reclaim=false; ack(a,p.e,m.acc,m.c); a.inflight=0;} else lose(a,p.e,m.fin,m.cur!==0); } } }
    else if(m.t==='Err'){ if(a.pending&&a.pending.id===m.id){ a.pending=null; a.retry=true; } for(const x of a.fin.values()) if(x.pend===m.id){ x.pend=null; x.retry=true; } }
    reeval(a);
  }
  function timers(){ for(const b of L){ b.retry=false; if(b.sendTimer){ b.sendTimer=false; b.inflight=0; }
      for(const x of b.fin.values()){ x.retry=false; if(x.pend && !net.some(e=>e.m.id===x.pend && (e.from===b.id||e.to===b.id))) x.pend=null; }
      if(b.pending && !net.some(e=>e.m.id===b.pending.id && (e.from===b.id||e.to===b.id))){ b.pending=null; b.retry=true; }
      if(b.cur && b.inflight>0 && !net.some(e=>(e.m.t==='Write'&&e.from===b.id)||((['Ack','Gap','BP','Stale'].includes(e.m.t))&&e.to===b.id&&e.m.id))){ b.inflight=0; }
      reeval(b);} }
  let byteId=0;
  const STEPS=4000;
  for(let i=0;i<STEPS;i++){
    const r=R(); const a=L[Math.floor(R()*L.length)];
    if(r<0.08){ const b=++byteId; a.typed.push(b); if(a.cur){ a.seg.get(a.cur.e).bytes.push(b);} else a.unassigned.push(b); a.sup=false; a.probe=false; reeval(a); continue; }
    if(r<0.11){ a.focus=!a.focus; if(a.focus){a.sup=false;a.probe=false;} reeval(a); continue; }
    if(r<0.13){ a.conn++; net=net.filter(e=>!(e.to===a.id||e.from===a.id)); a.pending=null; a.retry=false; a.inflight=0; a.sendTimer=false; if(a.cur) a.needs_reclaim=true; if(a.sup) a.probe=true; for(const x of a.fin.values()) x.pend=null; reeval(a); continue; }
    if(r<0.2){ timers(); continue; }
    if(r<0.3){ writeOne(); continue; }
    if(!net.length) continue;
    const e=net.splice(Math.floor(R()*net.length),1)[0];
    if(e.to==='H') host(e); else if(e.conn===A[e.to].conn) author(A[e.to],e.m);
  }
  // quiet phase: X focused, Y unfocused, no more faults (no errors, no reconnects)
  opts = { ...opts, errRate: 0 };
  A.Y.focus=false; A.X.focus=true; A.X.sup=false; A.X.probe=false; L.forEach(reeval);
  for(let k=0;k<200000;k++){
    if(net.length){ const e=net.splice(Math.floor(R()*net.length),1)[0]; if(e.to==='H') host(e); else if(e.conn===A[e.to].conn) author(A[e.to],e.m); }
    else if(H.wq.length) writeOne();
    else { const before=JSON.stringify([A.X.cur,A.X.unassigned.length,A.X.fin.size,A.Y.fin.size,A.Y.cur]); timers(); if(!net.length && JSON.stringify([A.X.cur,A.X.unassigned.length,A.X.fin.size,A.Y.fin.size,A.Y.cur])===before) break; }
  }
  const bad=[]; const seen=new Set();
  for(const b of H.tap){ if(seen.has(b)) bad.push('dup '+b); seen.add(b); }
  for(const a of L){
    const w=H.tap.filter(b=>a.typed.includes(b)); let last=-1; for(const b of w){ const ix=a.typed.indexOf(b); if(ix<last) bad.push(`${a.id} order`); last=ix; }
    for(const b of a.typed){ const st=a.status.get(b); const written=seen.has(b);
      if(st==='W'&&!written) bad.push(`${a.id} ${b} claimed W not written`);
      if(st==='N'&&written) bad.push(`${a.id} ${b} claimed N but written`);
      if(st==='Q'&&!written) bad.push(`${a.id} ${b} Q never written`);
      const inCur=a.cur&&a.seg.get(a.cur.e).bytes.includes(b); const unass=a.unassigned.includes(b);
      if(!st && !written && !inCur && !unass) bad.push(`${a.id} ${b} lost`);
    }
    if(a.fin.size) bad.push(`${a.id} finalizing stuck ${[...a.fin.keys()]}`);
  }
  const X=A.X;
  if(X.unassigned.length) bad.push(`X unassigned left ${X.unassigned.length} cur=${JSON.stringify(X.cur)} pend=${JSON.stringify(X.pending)} sup=${X.sup} H=${H.holder}@${H.epoch}`);
  if(X.cur && unsent(X)>0) bad.push('X cur not fully accepted');
  if(A.Y.cur) bad.push('Y holds lease while unfocused');
  return bad.length?{seed,bad:bad.slice(0,5)}:null;
}
