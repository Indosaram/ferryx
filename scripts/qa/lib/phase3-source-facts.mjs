/**
 * Reads the constants and integration surfaces this harness depends on directly from the tree.
 *
 * Nothing here is a hardcoded guess about the wire: a value that cannot be read from its real
 * source is a typed failure, never a default. That matters most for protocol versions, where a
 * stale literal (6, 7, or a future bump) would make every case assert against a protocol the
 * binaries do not speak.
 */

import { readFile } from 'node:fs/promises';
import { join } from 'node:path';

export class SourceFactError extends Error {
  constructor(code, detail) {
    super(`${code}: ${detail}`);
    this.code = code;
    this.detail = detail;
  }
}

const FILES = {
  protocol: 'crates/ferryx-ptykeeper/src/protocol.rs',
  lib: 'crates/ferryx-ptykeeper/src/lib.rs',
  budget: 'crates/ferryx-ptykeeper/src/budget.rs',
  keeper: 'crates/ferryx-ptykeeper/src/keeper.rs',
  server: 'crates/ferryx-ptykeeper/src/server.rs',
  session: 'crates/ferryx-ptykeeper/src/session.rs',
  epoch: 'crates/ferryx-ptykeeper/src/epoch.rs',
  endpoint: 'crates/ferryx-ptykeeper/src/endpoint.rs',
  keeperError: 'crates/ferryx-ptykeeper/src/error.rs',
  checkpoint: 'crates/ferryx-ptykeeper/src/checkpoint.rs',
  crashSupervisor: 'src-tauri/src/daemon/crash_supervisor.rs',
  keeperLaunchd: 'src-tauri/src/daemon/keeper_launchd.rs',
  keeperStartup: 'src-tauri/src/daemon/keeper_startup.rs',
  keeperClient: 'src-tauri/src/daemon/keeper_client.rs',
  keeperSession: 'src-tauri/src/daemon/keeper_session.rs',
  daemonProtocol: 'src-tauri/src/daemon/protocol.rs',
  daemonMod: 'src-tauri/src/daemon/mod.rs',
  daemonServer: 'src-tauri/src/daemon/server.rs',
  sessionService: 'src-tauri/src/daemon/session_service.rs',
  terminalService: 'src-tauri/src/terminal/service.rs',
  terminalPty: 'src-tauri/src/terminal/pty.rs',
  cli: 'src-tauri/src/cli.rs',
  handover: 'src-tauri/src/daemon/handover.rs',
  handoverSocket: 'src-tauri/src/daemon/handover_socket.rs',
  handoverTransaction: 'src-tauri/src/daemon/handover_transaction.rs',
  rollback: 'src-tauri/src/daemon/rollback.rs',
};

async function read(root, key) {
  const path = join(root, FILES[key]);
  try {
    return await readFile(path, 'utf8');
  } catch (error) {
    throw new SourceFactError('SOURCE_UNREADABLE', `${path}: ${error.message}`);
  }
}

/**
 * `pub const NAME: <type> = <numeric expr>;`
 *
 * `matchAll` is used because a constant can be defined twice under `cfg` (the daemon protocol
 * version is): the first definition is the default-features one and is the one a normal build
 * speaks.
 */
function numericConst(text, name, source, index = 0) {
  const pattern = new RegExp(`pub const\\s+${name}\\s*:\\s*[A-Za-z0-9_]+\\s*=\\s*([^;]+);`, 'g');
  const matches = [...text.matchAll(pattern)];
  const match = matches[index];
  if (!match) throw new SourceFactError('SOURCE_CONST_ABSENT', `${source}: pub const ${name}`);
  return evalNumber(match[1], `${source}:${name}`);
}

/**
 * Rejects anything but integers, underscores and multiplication, so a source edit cannot smuggle code.
 *
 * Rust literals are decimal, `0x` hex or `0o` octal (`endpoint.rs` writes `DIR_MODE = 0o700`), so the
 * two radix prefixes are rewritten to decimal before the strict shape check. Anything else -- a path,
 * a `Duration`, an identifier -- still fails rather than being interpreted.
 */
function evalNumber(expr, label) {
  const cleaned = expr
    .replace(/_/g, '')
    .replace(/0[xX]([0-9a-fA-F]+)/g, (_match, digits) => String(parseInt(digits, 16)))
    .replace(/0[oO]([0-7]+)/g, (_match, digits) => String(parseInt(digits, 8)))
    .trim();
  if (!/^[\d\s*()]+$/.test(cleaned)) {
    throw new SourceFactError('SOURCE_CONST_UNPARSEABLE', `${label}: ${JSON.stringify(expr)}`);
  }
  const value = Function(`"use strict";return (${cleaned});`)();
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new SourceFactError('SOURCE_CONST_UNPARSEABLE', `${label}: ${JSON.stringify(expr)} -> ${value}`);
  }
  return value;
}

function numberConsts(text, name) {
  const pattern = new RegExp(`pub const\\s+${name}\\s*:\\s*u32\\s*=\\s*(\\d+);`, 'g');
  return [...text.matchAll(pattern)].map((match) => Number(match[1]));
}

/**
 * `&str` and `&'static str` are both used in this tree (`crash_supervisor.rs` writes the short form),
 * so the lifetime is optional rather than assumed present.
 */
function stringConst(text, name, source) {
  const pattern = new RegExp(`pub const\\s+${name}\\s*:\\s*&(?:'static\\s+)?str\\s*=\\s*"([^"]*)";`);
  const match = pattern.exec(text);
  if (!match) throw new SourceFactError('SOURCE_CONST_ABSENT', `${source}: pub const ${name}`);
  return match[1];
}

function byteStringConst(text, name, source) {
  const pattern = new RegExp(`pub const\\s+${name}\\s*:\\s*\\[u8;\\s*\\d+\\]\\s*=\\s*\\*b"([^"]*)";`);
  const match = pattern.exec(text);
  if (!match) throw new SourceFactError('SOURCE_CONST_ABSENT', `${source}: pub const ${name}`);
  return match[1];
}

function durationMillis(text, name, source) {
  const pattern = new RegExp(`pub const\\s+${name}\\s*:[^=]+=\\s*Duration::from_millis\\((\\d+)\\)`);
  const match = pattern.exec(text);
  if (!match) throw new SourceFactError('SOURCE_CONST_ABSENT', `${source}: pub const ${name}`);
  return Number(match[1]);
}

const count = (text, re) => (text.match(re) ?? []).length;

/**
 * Reads every constant the harness mirrors, plus the integration surfaces the plan's Phase 3 cases
 * need. `surfaces` is returned as measured data, not as a verdict: each case decides what an absent
 * surface means for it, and an absent one always becomes a typed gap rather than a pass.
 */
export async function readSourceFacts(root) {
  const [
    protocol,
    lib,
    budget,
    keeperSrc,
    server,
    session,
    epoch,
    endpoint,
    crashSupervisor,
    keeperLaunchd,
    keeperStartup,
    keeperClient,
    daemonProtocol,
    daemonMod,
    daemonServer,
    sessionService,
    terminalService,
    terminalPty,
    cli,
    handover,
    handoverSocket,
    handoverTransaction,
    rollback,
    keeperError,
    keeperSession,
    checkpointSrc,
  ] = await Promise.all([
    read(root, 'protocol'),
    read(root, 'lib'),
    read(root, 'budget'),
    read(root, 'keeper'),
    read(root, 'server'),
    read(root, 'session'),
    read(root, 'epoch'),
    read(root, 'endpoint'),
    read(root, 'crashSupervisor'),
    read(root, 'keeperLaunchd'),
    read(root, 'keeperStartup'),
    read(root, 'keeperClient'),
    read(root, 'daemonProtocol'),
    read(root, 'daemonMod'),
    read(root, 'daemonServer'),
    read(root, 'sessionService'),
    read(root, 'terminalService'),
    read(root, 'terminalPty'),
    read(root, 'cli'),
    read(root, 'handover'),
    read(root, 'handoverSocket'),
    read(root, 'handoverTransaction'),
    read(root, 'rollback'),
    read(root, 'keeperError'),
    read(root, 'keeperSession'),
    read(root, 'checkpoint'),
  ]);

  // Two definitions exist: default features, then a QA-only bump. The first is what a normal build
  // speaks; the second is recorded so the mismatch path is visible rather than guessed.
  const daemonVersions = numberConsts(daemonProtocol, 'DAEMON_PROTOCOL_VERSION');
  if (!daemonVersions.length) {
    throw new SourceFactError('SOURCE_CONST_ABSENT', `${FILES.daemonProtocol}: pub const DAEMON_PROTOCOL_VERSION`);
  }

  const keeper = {
    magic: byteStringConst(protocol, 'MAGIC', FILES.protocol),
    headerBytes: numericConst(protocol, 'HEADER_BYTES', FILES.protocol),
    framingVersion: numericConst(protocol, 'FRAMING_VERSION', FILES.protocol),
    maxFramePayloadBytes: numericConst(protocol, 'MAX_FRAME_PAYLOAD_BYTES', FILES.protocol),
    maxConnections: numericConst(protocol, 'MAX_CONNECTIONS', FILES.protocol),
    maxInflightFrames: numericConst(protocol, 'MAX_INFLIGHT_FRAMES', FILES.protocol),
    attachInlineMaxBytes: numericConst(protocol, 'ATTACH_INLINE_MAX_BYTES', FILES.protocol),
    maxCheckpointBytes: numericConst(checkpointSrc, 'MAX_CHECKPOINT_BYTES', FILES.checkpoint),
    noRequestId: stringConst(protocol, 'NO_REQUEST_ID', FILES.protocol),
    protocolMajor: numericConst(lib, 'PROTOCOL_MAJOR', FILES.lib),
    protocolMinor: numericConst(lib, 'PROTOCOL_MINOR', FILES.lib),
    replaySessionBytes: numericConst(lib, 'REPLAY_SESSION_BYTES', FILES.lib),
    budgetAggregateBytes: numericConst(budget, 'BUDGET_AGGREGATE_BYTES', FILES.budget),
    replayAggregateBytes: numericConst(budget, 'REPLAY_AGGREGATE_BYTES', FILES.budget),
    sessionReservationBytes: numericConst(keeperSrc, 'SESSION_RESERVATION_BYTES', FILES.keeper),
    maxWriteBytes: numericConst(keeperSrc, 'MAX_WRITE_BYTES', FILES.keeper),
    responseBytesPerRequest: numericConst(server, 'RESPONSE_BYTES_PER_REQUEST', FILES.server),
    replayChunkBytes: numericConst(server, 'REPLAY_CHUNK_BYTES', FILES.server),
    outputChunkBytes: numericConst(server, 'OUTPUT_CHUNK_BYTES', FILES.server),
    outputPollMillis: durationMillis(server, 'OUTPUT_POLL', FILES.server),
    socketName: stringConst(crashSupervisor, 'KEEPER_SOCKET_NAME', FILES.crashSupervisor),
    qaLabel: stringConst(keeperLaunchd, 'QA_PLIST_LABEL', FILES.keeperLaunchd),
    productionLabel: stringConst(keeperLaunchd, 'PLIST_LABEL', FILES.keeperLaunchd),
    plistPathEnv: stringConst(keeperLaunchd, 'PLIST_PATH_ENV', FILES.keeperLaunchd),
    installRootEnv: stringConst(keeperLaunchd, 'INSTALL_ROOT_ENV', FILES.keeperLaunchd),
    maxSocketPathBytes: numericConst(endpoint, 'MAX_SOCKET_PATH_BYTES', FILES.endpoint),
    dirMode: numericConst(endpoint, 'DIR_MODE', FILES.endpoint),
    socketMode: numericConst(endpoint, 'SOCKET_MODE', FILES.endpoint),
    // `SessionOwner::as_str`: the two labels `OWNERSHIP_LIST` reports.
    ownerLabels: [...session.matchAll(/Self::(?:Keeper|Daemon) => "([a-z]+)"/g)].map((match) => match[1]),
    // Every verb the keeper answers, so a case cannot invent one.
    kinds: [...protocol.matchAll(/^\s{4}([A-Z][A-Za-z]+) = (0x[0-9a-f]{4}),$/gm)].map((match) => ({
      name: match[1],
      value: Number(match[2]),
    })),
    errorCodes: [...keeperError.matchAll(/Self::[A-Za-z]+ => "([A-Z_]+)"/g)].map((match) => match[1]),
  };

  const daemon = {
    protocolVersion: daemonVersions[0],
    protocolVersionQaBump: daemonVersions.length > 1 ? daemonVersions[1] : null,
    spawnRequestType: /Spawn\s*\{/.test(daemonProtocol),
    handshakeRequestType: /Handshake\s*\{/.test(daemonProtocol),
    attachRequestType: /Attach\s*\{/.test(daemonProtocol),
    describeLiveSessionsType: /DescribeLiveSessions/.test(daemonProtocol),
    // `describe_live_sessions` reads `started_at_unix_ms`, so `localChildStartTime` is milliseconds.
    liveStartTimeIsMillis: /started_at_unix_ms/.test(terminalService),
  };

  // The real daemon spawn path: the files a session is opened through. `keeper_session.rs` is kept
  // out of the scan below because the routing decision lives one layer up: `terminal/pty.rs` asks the
  // installed runtime whether new sessions route to the keeper, so counting the module that implements
  // the runtime here would double-count a single seam.
  const spawnPath = [daemonServer, sessionService, terminalService, terminalPty];
  const keeperClientReferencingFiles = Object.entries({
    [FILES.crashSupervisor]: crashSupervisor,
    [FILES.daemonServer]: daemonServer,
    [FILES.sessionService]: sessionService,
    [FILES.terminalService]: terminalService,
    [FILES.terminalPty]: terminalPty,
    [FILES.keeperSession]: keeperSession,
    [FILES.cli]: cli,
  })
    // `crash_supervisor.rs` reaches the keeper through fully-qualified `keeper_client::` paths rather
    // than importing `KeeperClient`, so both spellings count as a real reference.
    .filter(([, text]) => /KeeperClient|keeper_client::/.test(text))
    .map(([path]) => path);

  const surfaces = {
    // Whether the daemon names the keeper client in the path a session is spawned through. Routing
    // itself is decided at runtime by `terminal/pty.rs`, not by these counts, so these are recorded
    // as a description of the tree rather than used as a verdict by any case.
    keeperClientApiVerbs: count(keeperClient, /\n\s*pub fn /g),
    keeperClientReferencingFiles,
    spawnPathMentionsKeeperClient: spawnPath.some((text) => /KeeperClient|keeper_client::/.test(text)),
    keeperSpawnCallSitesInSpawnPath: spawnPath.reduce(
      (total, text) => total + count(text, /KeeperClient::connect|keeper\s*\.\s*spawn\(/g),
      0,
    ),
    // A per-session ownership flag the daemon flips. Absent means no mixed-ownership state exists.
    // `keeper_session.rs` defines it (`keeper_ownership_enabled`); the surface only matters if a
    // *wired* module consults it, which `keeperSessionWired` answers separately.
    ownershipFlagPresent: /keeper_ownership|use_keeper|keeperOwnerEnabled|KEEPER_OWNERSHIP/.test(
      [daemonServer, sessionService, terminalService, keeperSession, cli].join('\n'),
    ),
    // The decisive wiring question: is the keeper integration reachable from the running daemon at
    // all? Merely *naming* the module is not wiring -- `terminal/service.rs` declares an
    // `install_keeper` method and a `keeper()` accessor that nothing calls, which routes no session.
    // So a file counts only when it actually invokes the runtime, not when it imports the type.
    keeperSessionWired: Object.entries({
      [FILES.daemonServer]: daemonServer,
      [FILES.sessionService]: sessionService,
      [FILES.terminalService]: terminalService,
      [FILES.terminalPty]: terminalPty,
      [FILES.cli]: cli,
      [FILES.handover]: handover,
      // The one file that installs the runtime into a live daemon. Leaving it out made the whole
      // wiring measurement depend on `handover.rs`'s `terminals.keeper()` read alone, which would
      // report "no wiring" on a tree where the desktop start pass installs the keeper perfectly.
      [FILES.keeperStartup]: keeperStartup,
    })
      // `fn install_keeper(` is the definition, not a call, so the lookbehind keeps the declaration
      // itself from counting as the call site that would prove wiring.
      .filter(([, text]) => /(?<!fn )install_keeper\(|\.keeper\(\)|KeeperRuntime::(?:spawn|attach|connect)/.test(text))
      .map(([path]) => path),
    // A live-FD migration verb in either wire: the only way one session can change owner.
    fdMigrationPresent: /MigrateMaster|MigrateFd|MigrateOwnership|migrate_master/.test(
      [protocol, keeperClient, daemonServer].join('\n'),
    ),
    // A rollback surface a user can actually invoke. `rollback_gate` is a pure function today.
    rollbackGateDefined: /pub fn rollback_gate\(/.test(keeperLaunchd),
    // Enable and disable are different seams and must not be measured as one. Enable is the desktop
    // start pass flipping the flag ON (`activate_new_sessions` -> `activate_ownership`); disable is
    // `reconcile_and_activate` writing it OFF when `OwnershipPolicy` reads
    // `FERRYX_KEEPER_OWNERSHIP=disabled`. `keeper_session.rs` is left out of the disable scan on
    // purpose -- its only `set_keeper_enabled(false)` occurrences are inside its own `#[cfg(test)]`
    // module, and counting those would report a production disable path that does not exist.
    ownershipEnableSurface:
      /pub fn activate_new_sessions\(/.test(keeperSession) && /\.activate_new_sessions\(\)/.test(keeperStartup),
    ownershipDisableSurface: /(?<!fn )set_keeper_enabled\(false\)/.test(
      [daemonServer, cli, keeperStartup].join('\n'),
    ),
    // The production start pass: the daemon's own `install_keeper(` call site. A rollback verb can
    // only be reached from a daemon if this pass ran, so it is measured separately from the pure gate.
    keeperStartupInstallsRuntime: /terminal_service\.install_keeper\(/.test(keeperStartup),
    keeperStartupReconciles: /pub fn reconcile\(/.test(keeperStartup),
    // Whether the keeper start pass is macOS-only in SOURCE. `keeper_startup` is declared behind
    // `#[cfg(target_os = "macos")]` in `daemon/mod.rs` and its one production call site in
    // `daemon/server.rs` is gated the same way, so a Linux build contains no start pass at all: no
    // `launchctl` ownership exists off macOS and the keeper is introduced on macOS first (plan line
    // 240). These are source facts; the host they are combined with is the harness's own
    // `process.platform`, so neither half is guessed.
    keeperStartupIsMacosGated: /#\[cfg\(target_os = "macos"\)\]\s*\n\s*pub mod keeper_startup;/.test(daemonMod),
    keeperStartupCallSiteIsMacosGated:
      /#\[cfg\(target_os = "macos"\)\]\s*\n\s*\{[^}]*keeper_startup::start_production/.test(daemonServer),
    // `daemon/rollback.rs` is the module that actually decides a rollback, so it is where the gate's
    // production call site lives; leaving it out reported "no call site" on a tree that has one.
    rollbackGateCallSitesOutsideDefinition: [daemonServer, cli, keeperClient, rollback].reduce(
      (total, text) => total + count(text, /rollback_gate\(|rollback_refusal_message\(/g),
      0,
    ),
    // `ferryx-cli service rollback` is a version downgrade, NOT the mixed-ownership gate: it parses a
    // version and refuses with `SERVICE_ROLLBACK_UNAVAILABLE` without consulting the gate at all.
    // Counting it here would let the plan's mixed-ownership requirement be satisfied by a verb that
    // never looks at who owns a session, so the two are measured apart.
    rollbackCliVerb: /"(rollback|downgrade)"/.test(cli),
    rollbackCliReachesGate: /run_rollback\(/.test(cli) && /rollback_gate\(/.test(rollback),
    // The graceful handover a normal app replacement uses: the three daemon verbs plus the CLI flag
    // a successor is started with. These are what the graceful case drives, and none of them depends
    // on the keeper, so they are measured separately from keeper routing.
    handoverVerbsPresent:
      /PrepareHandover/.test(daemonProtocol) &&
      /TransferSessions/.test(daemonProtocol) &&
      /CommitHandover/.test(daemonProtocol) &&
      /--handover-from/.test(cli),
    // Whether the graceful handover path knows keeper-owned sessions exist at all.
    handoverKeeperMentions:
      count(handover, /keeper/gi) + count(handoverSocket, /keeper/gi) + count(handoverTransaction, /keeper/gi),
    // The keeper originates OUTPUT frames on an attached connection, so an attached reader must
    // tolerate unsolicited frames arriving between its own replies.
    keeperOriginatesOutput: /Kind::Output,/.test(server) && /fn output_run\(/.test(server),
    // Per-session epoch fencing: the gate that refuses a stale read or write.
    epochFenceDefined: /pub fn fence\(/.test(epoch),
  };

  return { keeper, daemon, surfaces, sources: FILES };
}
