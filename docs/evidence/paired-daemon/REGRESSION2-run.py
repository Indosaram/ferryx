import json, os, pathlib, subprocess, tempfile, time

root = pathlib.Path('/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8')
evidence = root / 'docs/evidence/paired-daemon'
qa = pathlib.Path(tempfile.mkdtemp(prefix='fix3-st_01a099da-', dir='/tmp'))
env = os.environ.copy()
env['PATH'] = '/Users/indo/.cargo/bin:/Users/indo/.local/bin:/Users/indo/.bun/bin:/opt/homebrew/bin:/usr/bin:/bin'
for key, name in {'HOME':'home','FERRYX_RUNTIME_DIR':'runtime','FERRYX_DATA_DIR':'data','FERRYX_SESSION_DIR':'sessions','XDG_CONFIG_HOME':'config','XDG_DATA_HOME':'xdg-data','XDG_CACHE_HOME':'cache','XDG_RUNTIME_DIR':'xdg-runtime','TMPDIR':'tmp','CODEX_HOME':'codex','CLAUDE_CONFIG_DIR':'claude'}.items():
    path = qa / name
    path.mkdir(mode=0o700)
    env[key] = str(path)
env.update(CARGO_HOME='/Users/indo/.cargo', RUSTUP_HOME='/Users/indo/.rustup', CARGO_TARGET_DIR=str(root/'src-tauri/target'), CARGO_BUILD_JOBS='4', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0', CARGO_INCREMENTAL='0', RUSTC_WRAPPER='', GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=str(qa/'home/.gitconfig'), FERRYX_AGENT_STATE_SOCKET=str(qa/'runtime/agent.sock'))
flags = ['--locked', '--manifest-path', 'src-tauri/Cargo.toml', '--no-default-features']
commands = [
    ('full', ['cargo','test']+flags+['--lib','--','--test-threads=1']),
    ('paired-host', ['cargo','test']+flags+['--lib','paired_host::','--','--test-threads=1']),
    ('check-lib', ['cargo','check']+flags+['--lib']),
    ('check-bins', ['cargo','check']+flags+['--bin','ferryx-cli','--bin','ferryx-relay']),
    ('build-entrypoint', ['cargo','build']+flags+['--bin','ferryx-relay']),
]
results = []
for label, command in commands:
    log = evidence / ('REGRESSION2-' + label + '-' + str(os.getpid()) + '.log')
    started = time.monotonic()
    print('RUN', label, log, flush=True)
    with log.open('w') as output:
        output.write('$ ' + ' '.join(command) + '\n'); output.flush()
        result = subprocess.run(command, cwd=root, env=env, stdout=output, stderr=subprocess.STDOUT)
        output.write('\nEXIT=' + str(result.returncode) + '\n')
    results.append(dict(label=label, exit=result.returncode, seconds=time.monotonic()-started, log=str(log)))
    print('RESULT', results[-1], flush=True)
(evidence/'REGRESSION2-results.json').write_text(json.dumps(dict(qaRoot=str(qa), results=results), indent=2)+'\n')
if all(result['exit'] == 0 for result in results):
    (evidence/'REGRESSION2-GREEN.log').write_text(''.join(pathlib.Path(result['log']).read_text() for result in results))
raise SystemExit(any(result['exit'] for result in results))
