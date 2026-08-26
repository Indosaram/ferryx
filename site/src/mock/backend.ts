import type { BrowserSessionSummary, BrowserState, DirtyState, Worktree, TerminalOutputPayload, TerminalLifecyclePayload } from '@ui/lib/types';

type Listener = (payload: unknown) => void;
const listeners = new Map<string, Set<Listener>>();
export function on(event: string, handler: Listener) { if (!listeners.has(event)) listeners.set(event, new Set()); listeners.get(event)!.add(handler); return () => listeners.get(event)?.delete(handler); }
export function emitEvent(event: string, payload: unknown) { listeners.get(event)?.forEach((handler) => handler(payload)); }

const worktrees: Worktree[] = [
  { path: '.', head: 'a1b2c3d', branch: 'main', bare: false, detached: false, locked: null, prunable: null },
  { path: '../ferryx-feature-live-demo', head: 'd4e5f6a', branch: 'feature/live-demo', bare: false, detached: false, locked: null, prunable: null },
];
const sessions = new Map<string, { cwd: string; line: string }>();
const browsers = new Map<string, BrowserState>();
let nextId = 1;
const id = (prefix: string) => `${prefix}-${nextId++}`;
const argsOf = (args: unknown) => (args && typeof args === 'object' ? args as Record<string, any> : {});
const prompt = (s: { cwd: string }) => `ferryx on git:${s.cwd === '.' ? 'main' : 'feature/live-demo'} % `;

// Real PTY protocol streams terminal_output payloads as base64 UTF-8; match it.
function toBase64Utf8(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return globalThis.btoa(binary);
}
function emitOutput(sessionId: string, text: string) {
  emitEvent('terminal_output', { sessionId, data: toBase64Utf8(text) } satisfies TerminalOutputPayload);
}

function shell(session: { cwd: string; line: string }, command: string) {
  const [name, ...rest] = command.trim().split(/\s+/); const arg = rest.join(' ');
  if (name === 'cd' && arg) { session.cwd = arg; return ''; }
  if (name === 'ls') return 'README.md  package.json  src  ui  site';
  if (name === 'pwd') return `/workspace/ferryx/${session.cwd === '.' ? '' : session.cwd}`;
  if (name === 'git' && rest[0] === 'status') return 'On branch main\nnothing to commit, working tree clean';
  if (name === 'git' && rest[0] === 'branch') return '* main\n  feature/live-demo';
  if (name === 'git' && rest[0] === 'log') return 'a1b2c3d (HEAD -> main) Live demo\n98f7e6d Initial commit';
  if (name === 'cargo' && rest[0] === 'test' || name === 'bun' && rest[0] === 'test') return 'running 8 tests\ntest result: ok. 8 passed; 0 failed';
  if (name === 'echo') return arg;
  if (name === 'help') return 'Commands: ls, pwd, git status, git branch, git log --oneline, cargo test, bun test, echo, clear';
  if (name === 'clear') return '\u001b[2J\u001b[H';
  return `${command}: command not found`;
}

export async function invoke(command: string, rawArgs?: unknown): Promise<unknown> {
  const a = argsOf(rawArgs); const request = argsOf(a.request);
  switch (command) {
    case 'cmd_project_register': return { workspaceId: request.workspaceId, repoRoot: request.repoPath };
    case 'cmd_project_branches': return [{ name: 'main', isCurrent: true }, { name: 'feature/live-demo', isCurrent: false }];
    case 'cmd_worktree_list': return worktrees;
    case 'cmd_worktree_status': { const result: DirtyState = { isDirty: false, files: [] }; return result; }
    case 'cmd_worktree_create': return { ...worktrees[1], path: `../${request.worktree?.slug ?? 'new-worktree'}` };
    case 'cmd_worktree_delete': case 'cmd_worktree_delete_destructive': case 'cmd_worktree_resize': return undefined;
    case 'cmd_worktree_delete_preview': return { branch: request.worktree?.slug ?? 'feature/live-demo', head: 'd4e5f6a', upstream: null, merged: false, ahead: 0, behind: 0 };
    case 'cmd_terminal_spawn': { const sessionId = id('terminal'); const s = { cwd: request.cwd || '.', line: '' }; sessions.set(sessionId, s); emitEvent('terminal_lifecycle', { sessionId, state: 'started', exitCode: null, reason: null } satisfies TerminalLifecyclePayload); emitOutput(sessionId, prompt(s)); return { sessionId }; }
    case 'cmd_terminal_list': return [...sessions].map(([sessionId, s]) => ({ sessionId, worktreePath: s.cwd }));
    case 'cmd_terminal_write': { const s = sessions.get(a.sessionId); if (!s) throw new Error(`Unknown terminal session: ${a.sessionId}`); for (const ch of String(a.data ?? '')) { if (ch === '\n' || ch === '\r') { const output = shell(s, s.line); emitOutput(a.sessionId, output ? `${ch}${output}\r\n${prompt(s)}` : `${ch}${prompt(s)}`); s.line = ''; } else if (ch === '\u007f') { s.line = s.line.slice(0, -1); } else if (ch !== '\b') { s.line += ch; emitOutput(a.sessionId, ch); } } return undefined; }
    case 'cmd_terminal_get_cwd': { const s = sessions.get(a.sessionId); return { cwd: s?.cwd ?? '.' }; }
    case 'cmd_terminal_resize': case 'cmd_terminal_signal': return undefined;
    case 'cmd_terminal_close': sessions.delete(a.sessionId); return undefined;
    case 'cmd_terminal_preferences': return { fontFamily: 'monospace', fontSize: 13, macosOptionAsAlt: false, cursorStyle: 'block', theme: {}, source: 'defaults', status: 'absent', sourcePath: null };
    // The web demo has no libghostty compositor; acknowledge the surface commands so
    // NativeTerminalPane does not surface an attach failure over the preview.
    case 'cmd_native_terminal_attach': case 'cmd_native_terminal_set_focus': case 'cmd_native_terminal_set_bounds': return { cursorCol: 0, cursorRow: 0, cellWidthPx: 8, cellHeightPx: 17 };
    case 'cmd_native_terminal_detach': case 'cmd_native_terminal_send_input': case 'cmd_native_terminal_scroll': return undefined;
    case 'cmd_native_terminal_copy_selection': return null;
    case 'cmd_browser_create': { const browserId = id('browser'); const b: BrowserState = { browserId, webviewLabel: browserId, workspaceId: request.workspaceId, worktreePath: request.worktreePath, profileId: request.profile ?? 'default', generation: 0, url: request.url, title: request.url, loading: false, canGoBack: false, canGoForward: false, zoomFactor: 1, loadError: null, visible: request.visible ?? true }; browsers.set(browserId, b); return b; }
    case 'cmd_browser_navigate': { const b = browsers.get(a.browserId); if (b) { b.url = a.url; b.title = a.url; b.generation++; } return undefined; }
    case 'cmd_browser_reload': { const b = browsers.get(a.browserId); if (b) b.generation++; return undefined; }
    case 'cmd_browser_get_state': return browsers.get(a.browserId);
    case 'cmd_browser_list': return [...browsers.values()].map((b): BrowserSessionSummary => ({ browserId: b.browserId, webviewLabel: b.webviewLabel, workspaceId: b.workspaceId, url: b.url, title: b.title, visible: b.visible, profileId: b.profileId }));
    case 'cmd_browser_set_bounds': return undefined;
    case 'cmd_browser_set_visible': { const b = browsers.get(a.browserId); if (b) b.visible = a.visible; return undefined; }
    case 'cmd_browser_set_zoom': { const b = browsers.get(a.browserId); if (b) b.zoomFactor = a.zoomFactor; return a.zoomFactor; }
    case 'cmd_browser_focus': return undefined;
    case 'cmd_browser_close': browsers.delete(a.browserId); return undefined;
    case 'cmd_browser_open_external': return undefined;
    case 'cmd_remote_status': return { enabled: false, mode: 'off', port: 43821, boundAddress: null, localIp: null, tailscale: { installed: false, running: false, tailnetName: null, selfDns: null, serveActive: false } };
    case 'cmd_remote_enable': case 'cmd_remote_disable': return invoke('cmd_remote_status');
    case 'cmd_remote_pairing_create': return { code: '4F2A-91', expiresInSeconds: 300 };
    case 'cmd_remote_devices': return []; case 'cmd_remote_device_revoke': return false;
    case 'cmd_tailscale_status': return { installed: false, running: false, tailnetName: null, selfDns: null, serveActive: false };
    default: throw new Error(`Mock backend does not implement IPC command: ${command}`);
  }
}
