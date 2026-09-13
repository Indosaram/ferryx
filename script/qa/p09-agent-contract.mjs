// Owned, std-only production-seam runner; never opens an ambient profile or daemon.
import { readFileSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
const root = resolve(import.meta.dirname, '../..');
const read = p => readFileSync(join(root, p), 'utf8');
const section = (s, begin, end) => {
  if (s.split(begin).length !== 2 || s.split(end).length !== 2) throw Error('nonunique production markers');
  return s.slice(s.indexOf(begin), s.indexOf(end));
};
const agents = read('src-tauri/src/ipc/agents.rs');
const ext = read('src-tauri/src/daemon/agent_extension.rs');
const dir = mkdtempSync(join(tmpdir(), 'ferryx-p09-'));
try {
  const source = `use std::path::{Path,PathBuf};\n${section(agents, 'fn resolve_binary_with_env(', '#[cfg(test)]\nmod tests')}
mod extension {
use std::path::{Path,PathBuf};
pub const EXTENSION_SOURCE: &str = include_str!(${JSON.stringify(join(root, 'src-tauri/resources/agent-extensions/ferryx-agent-state.ts'))});
pub const EXTENSION_FILE_NAME: &str = "ferryx-agent-state.ts";
${section(ext, 'fn extension_dirs_with_env(', 'pub fn install_agent_state_extension()')}
${ext.slice(ext.indexOf('#[cfg(test)]\nmod p09_tests'))}
}
${agents.slice(agents.indexOf('#[cfg(test)]\nmod p09_tests'))}`;
  if (!agents.includes('mod p09_tests') || !ext.includes('mod p09_tests')) throw Error('missing tests');
  writeFileSync(join(dir, 'contract.rs'), source);
  const compile = spawnSync('rustc', ['--edition=2021', '--test', join(dir, 'contract.rs'), '-o', join(dir, 'contract')], { stdio: 'inherit' });
  if (compile.status !== 0) throw Error('compile prerequisite failed, not behavioral RED');
  const run = spawnSync(join(dir, 'contract'), ['--nocapture'], { stdio: 'inherit', timeout: 30000 });
  if (run.error) throw run.error;
  process.exitCode = run.status ?? 1;
} finally {
  rmSync(dir, { recursive: true, force: true });
  console.log('P09 owned executable/source/fixture root removed:', dir);
}
