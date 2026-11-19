import { execFile } from 'node:child_process';
import { constants } from 'node:fs';
import { chmod, copyFile, mkdir, readdir, realpath, stat } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { LIBRARIES } from '../../../stage-ffmpeg.mjs';
import { GuardError } from './guards.mjs';

const exec = promisify(execFile);
const soname = new RegExp(`^lib(${LIBRARIES.join('|')})\\.so\\.[0-9]+$`);
export const isLinuxQaLibrary = (name) => soname.test(name);

/** The SDK contains both SONAME symlinks and full-version files. Ship exactly one SONAME per library. */
export function assertLinuxQaLibraries(names) {
  const families = names.map((name) => soname.exec(name)?.[1]);
  if (names.length !== LIBRARIES.length || !LIBRARIES.every((lib) => families.includes(lib))) {
    throw new GuardError('QA_LINUX_FFMPEG_RUNTIME', `expected one version of each FFmpeg SONAME; found ${names.join(', ')}`);
  }
}

/** Only the producer needs patchelf. Edit private copies, never the SDK or the Cargo target. */
export async function stageLinuxQaLibraries({ executable, worktree, required = true, sdk = process.env.FFMPEG_DIR }) {
  const patch = async (...args) => (await exec('patchelf', args, { timeout: 30_000 })).stdout.trim();
  const needed = async (elf) => (await patch('--print-needed', elf)).split('\n').filter(isLinuxQaLibrary);
  const direct = await needed(executable);
  if (!direct.length) {
    if (required) throw new GuardError('QA_LINUX_FFMPEG_RUNTIME', `no linked FFmpeg libraries in ${executable}`);
    return; // A historical baseline or handover build may predate remote-desktop.
  }
  const sdkLib = await realpath(path.resolve(worktree, sdk || 'src-tauri/resources/ffmpeg/sdk', 'lib'));
  const names = (await readdir(sdkLib)).filter(isLinuxQaLibrary).sort();
  assertLinuxQaLibraries(names);
  const mustResolve = new Set(direct);
  const dest = path.join(path.dirname(executable), 'ffmpeg');
  await mkdir(dest, { mode: 0o700 });
  for (const name of names) {
    const source = await realpath(path.join(sdkLib, name));
    const input = await stat(source);
    if (path.dirname(source) !== sdkLib || !input.isFile() || await patch('--print-soname', source) !== name) {
      throw new GuardError('QA_LINUX_FFMPEG_RUNTIME', `invalid SDK library for ${name}: ${source}`);
    }
    const out = path.join(dest, name);
    await copyFile(source, out, constants.COPYFILE_EXCL);
    await chmod(out, (input.mode & 0o777) | 0o600);
    await patch('--set-rpath', '$ORIGIN', out); // Indirect dependencies must move with their caller too.
    for (const dependency of await needed(out)) mustResolve.add(dependency);
  }
  for (const name of mustResolve) if (!names.includes(name)) throw new GuardError('QA_LINUX_FFMPEG_RUNTIME', `SDK does not contain linked ${name}`);
  // The executable and a later staged daemon both look here. Drop the debug build's SDK path.
  const current = (await patch('--print-rpath', executable)).split(':').filter(Boolean);
  const rpath = ['$ORIGIN/ffmpeg', ...current.filter((entry) => entry !== '$ORIGIN/ffmpeg' && entry !== sdkLib)];
  await patch('--set-rpath', rpath.join(':'), executable);
}
