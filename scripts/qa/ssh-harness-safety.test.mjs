import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFile } from 'node:fs/promises';

const drivers = ['ssh-bridge-survival', 'ssh-helper-setup', 'ssh-helper-survival', 'ssh-process-survival', 'verify-ferryx-resume-cwd'];
// Link real driver source to denied boundary modules. Evaluation must not reach
// any host operation; unlike importing the old drivers this cannot launch QA.
for (const name of drivers) test(`import ${name} has no host effects`, async () => {
  const source = await readFile(new URL(`./${name}.mjs`, import.meta.url), 'utf8');
  const calls = [];
  const denied = (...args) => { calls.push(args); throw new Error('HOST_OPERATION'); };
  const context = vm.createContext({ console, Buffer, process: { platform: 'darwin', env: {}, argv: [] }, setTimeout: denied, clearTimeout() {} });
  const module = new vm.SourceTextModule(source, { context, identifier: new URL(`./${name}.mjs`, import.meta.url).href, initializeImportMeta(meta) { meta.url = new URL(`./${name}.mjs`, import.meta.url).href; meta.main = false; } });
  await module.link(async specifier => {
    const actual = await import(specifier);
    const keys = Object.keys(actual);
    const stub = new vm.SyntheticModule(keys, function () {
      for (const key of keys) this.setExport(key, specifier === 'node:assert/strict' ? actual[key] : typeof actual[key] === 'function' ? denied : actual[key]);
    }, { context });
    return stub;
  });
  let error;
  try { await module.evaluate(); } catch (e) { error = e; }
  assert.equal(calls.length, 0, `import invoked ${calls.length} host operations: ${error}`);
  assert.equal(error, undefined);
});

for (const name of ['ssh-bridge-survival', 'ssh-helper-setup']) test(`${name} never signals a detached endpoint PID`, async () => {
  const source = await readFile(new URL(`./${name}.mjs`, import.meta.url), 'utf8');
  const start = source.indexOf('async function reapPid(');
  const end = source.indexOf('\nfunction ', start) < 0 ? source.length : source.indexOf('\nfunction ', start);
  const next = source.indexOf('\nasync function ', start + 1);
  const code = source.slice(start, next > 0 && next < end ? next : end);
  const signals = [];
  const context = vm.createContext({ process: { kill(...args) { signals.push(args); const e = new Error('not owned'); e.code = 'ESRCH'; throw e; } }, console, isWin: false });
  vm.runInContext(code, context);
  await assert.rejects(vm.runInContext('reapPid(424242, "unproved")', context));
  assert.deepEqual(signals, []);
});

test('bridge initial marker accumulates fragmented output before acceptance', async () => {
  const source = await readFile(new URL('./ssh-bridge-survival.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('  // Initial Read on Connection 1');
  const end = source.indexOf('  console.log(JSON.stringify({', start);
  const chunks = ['INIT:non', 'ce:1\n'];
  let requests = 0;
  const context = vm.createContext({ assert, Buffer, target: {}, nonce: 'nonce', bridge1: { async request(op, p) { assert.equal(op, 'pty.read'); assert.equal(p.cursor, String(requests)); const text = chunks[requests++]; assert(text, 'read past marker'); return { cursor: String(requests), exited: false, gap: false, chunks: [{ data: Buffer.from(text).toString('base64') }] }; } } });
  const helperStart = source.indexOf('export async function readMarker(');
  if (helperStart >= 0) {
    const helperEnd = source.indexOf('\nexport async function main', helperStart);
    vm.runInContext(source.slice(helperStart, helperEnd).replace('export ', ''), context);
  }
  await vm.runInContext(`(async () => { ${source.slice(start, end)} })()`, context);
  assert.equal(requests, 2);
});

for (const name of ['ssh-bridge-survival', 'ssh-helper-setup']) test(`${name} refuses live detached launch without handle ownership`, async () => {
  // Boundary denies every host effect, including preflight and fixture creation.
  const source = await readFile(new URL(`./${name}.mjs`, import.meta.url), 'utf8');
  const context = vm.createContext({ process: { platform: 'darwin', env: {} }, assert, Error });
  const body = source.slice(source.indexOf('export async function main() {') + 30, source.indexOf('const repo ='));
  let error;
  try { await vm.runInContext(`(async () => { ${body} })()`, context); } catch (e) { error = e; }
  assert(error, 'unproved detached process launch must reject before fixture creation');
  assert.equal(error.code, 'QA_OWNERSHIP_PREREQUISITE');
});

test('process-survival authenticates socket before a destructive request', async () => {
  const source = await readFile(new URL('./ssh-process-survival.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('async function connect()');
  const end = source.indexOf('\nasync function rpc(', start);
  const sent = [];
  let destroyed = false;
  const socket = { on() {}, write(text) { sent.push(JSON.parse(text)); }, destroy() { destroyed = true; } };
  const context = vm.createContext({ assert, root: '/owned', join: (...v) => v.join('/'), sockets: new Set(), net: { createConnection: () => socket }, once: async () => {}, bounded: async p => p, readline: { createInterface: () => ({ [Symbol.asyncIterator]: () => ({ next: async () => ({ value: JSON.stringify({type:'handshakeOk',version:3,pid:99,epoch:7,binaryPath:'/owned/daemon'}), done:false }) }) }) }, daemon: {pid:42, exitCode:null, signalCode:null}, executable:'/owned/daemon', daemonEpoch:undefined, realpath:async p=>p });
  vm.runInContext(source.slice(start, end), context);
  await assert.rejects(vm.runInContext('connect()', context));
  assert.deepEqual(sent, [{type:'handshake',version:3}]);
  assert.equal(destroyed, true);
});

test('resume driver rejects mismatched handshake before registering or closing sessions', async () => {
  const source = await readFile(new URL('./verify-ferryx-resume-cwd.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('  assert.equal((await call({ type: "handshake"');
  const newStart = source.indexOf('  const identity = await call({ type: "handshake"');
  const end = source.indexOf('  const results = [];');
  const sent = [];
  const context = vm.createContext({ assert, repo:'/owned/project', binary:'/owned/daemon', daemon:{pid:42}, identityVerified:false, realpath:async p=>p, call:async r=> { sent.push(r.type); return {type:r.type==='handshake'?'handshakeOk':'registerWorkspaceOk',version:3,pid:99,epoch:7,binaryPath:'/owned/daemon'}; } });
  await assert.rejects(vm.runInContext(`(async()=>{ ${source.slice(start>=0?start:newStart,end)} })()`, context));
  assert.deepEqual(sent, ['handshake']);
});

test('helper-survival validates endpoint against owned daemon before destructive RPC', async () => {
  const source = await readFile(new URL('./ssh-helper-survival.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('  await bounded(ready, "helper ready");');
  const end = source.indexOf('  const script = process.platform', start);
  const requests = [];
  const context = vm.createContext({ assert, ready:Promise.resolve(), bounded:async p=>p, stateDir:'/owned/state', join:(...v)=>v.join('/'), readFile:async()=>JSON.stringify({pid:99,address:'127.0.0.1:1',token:'owned'}), daemon:{pid:42,exitCode:null,signalCode:null}, bridge:undefined, helperIdentity:undefined, projectDir:'/owned/project', connect:()=>({request:async op=>{requests.push(op);return {protocol:1,hostId:'qa-host',ownerId:'owner',epoch:'7'};}}) });
  await assert.rejects(vm.runInContext(`(async()=>{ ${source.slice(start,end)} })()`,context));
  assert.deepEqual(requests, []);
});

test('helper-survival retains fixture when owned child cleanup fails', async () => {
  const source = await readFile(new URL('./ssh-helper-survival.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('  const trackedPids = children.map');
  const end = source.indexOf('\n}\n\n}',start);
  let removed = false;
  const context = vm.createContext({ assert, console:{log(){},error(){}}, children:[{pid:42}], remoteShellPid:undefined, target:undefined, remoteStopped:false, fixture:'/owned', stop:async()=>{throw new Error('owned close failed');}, rm:async()=>{removed=true;}, process:{exitCode:0,kill(){const e=new Error('gone');e.code='ESRCH';throw e;}} });
  await assert.rejects(vm.runInContext(`(async()=>{ ${source.slice(start,end)} })()`,context));
  assert.equal(removed,false);
});

for (const name of ['ssh-bridge-survival', 'ssh-helper-setup']) test(`${name} retains detached fixture on unproved cleanup`, async () => {
  const source = await readFile(new URL(`./${name}.mjs`, import.meta.url), 'utf8');
  const start = source.lastIndexOf('} finally {');
  const end = source.indexOf('\n}\n\n}', start);
  let removed = false;
  const context = vm.createContext({ console:{log(){},error(){}}, fixture:'/owned', stateDir:'/owned/state', helperDaemonPid:42, daemonPid:42, ptyPid:null, target:null, bridge:null, ownedSshChildren:[], ownedChildren:[], reapPid:async()=>{throw new Error('unproved identity');}, reapChild:async()=>{}, rm:async()=>{removed=true;}, rmRetry:async()=>{removed=true;} });
  await assert.rejects(vm.runInContext(`(async()=>{ ${source.slice(start+11,end)} })()`,context));
  assert.equal(removed,false);
});

test('bridge marker accepts CRLF split UTF8 and rejects EOF, gap, wrong PID', async () => {
  const { readMarker } = await import('./ssh-bridge-survival.mjs');
  const bytes = Buffer.from('INIT:雪:1\r\n');
  const pieces = [bytes.subarray(0,6),bytes.subarray(6,8),bytes.subarray(8)];
  let reads=0;
  const connection={async request(op,p){assert.equal(op,'pty.read');assert.equal(p.cursor,String(reads));const data=pieces[reads++];assert(data);return {cursor:String(reads),pid:42,exited:false,gap:false,chunks:[{data:data.toString('base64')}]};}};
  assert.equal((await readMarker(connection,{},'0','INIT:雪:1\n',42)).cursor,'3');
  for(const reply of [{exited:true},{gap:true},{pid:99}]) {
    await assert.rejects(readMarker({async request(){return {cursor:'1',pid:42,exited:false,gap:false,chunks:[],...reply};}}, {}, '0','absent',42));
  }
});
