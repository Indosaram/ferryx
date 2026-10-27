"""Socket-free runner regressions; no compiler or Git processes are launched."""
import importlib.util
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
spec = importlib.util.spec_from_file_location('shard_runner', HERE / 'ci-test-shards.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

class ExecReached(BaseException):
    pass

class RunnerTests(unittest.TestCase):
    def wrapper(self, mode, args):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'wrapper.py'
            config = {'mode': mode, 'record': str(root / 'record.json'),
                      'expanded': str(root / 'expanded.rs'), 'count': 8, 'index': 3}
            runner.write_wrapper(path, config)
            seen = {}
            def execute(exe, argv):
                seen.update(exe=exe, args=argv, bootstrap=os.environ.get('RUSTC_BOOTSTRAP'))
                raise ExecReached()
            with patch.dict(os.environ, {}, clear=True), patch.object(sys, 'argv', [str(path), '/fake/rustc', *args]), patch('os.execv', side_effect=execute), patch('os.dup2'):
                with self.assertRaises(ExecReached):
                    runpy.run_path(str(path), run_name='__main__')
            seen['record'] = json.loads((root/'record.json').read_text()) if (root/'record.json').exists() else None
            return seen

    def test_inventory_replaces_all_emit_flags_and_scopes_bootstrap(self):
        args = ['--crate-name', 'ferryx_lib', '--test', '--emit=dep-info,link', '--emit', 'metadata', '--cfg', 'feature="desktop"']
        result = self.wrapper('inventory', args)
        self.assertEqual(result['bootstrap'], 'ferryx_lib')
        self.assertFalse(any(a.startswith('--emit') for a in result['args']))
        self.assertIn('-Zunpretty=expanded', result['args'])
        self.assertEqual(result['record']['original'], args)
        self.assertIn('feature="desktop"', result['args'])

    def test_shard_keeps_emit_and_does_not_bootstrap(self):
        args = ['--crate-name', 'ferryx_lib', '--test', '--emit=dep-info,link']
        result = self.wrapper('shard', args)
        self.assertIsNone(result['bootstrap'])
        self.assertIn('--emit=dep-info,link', result['args'])
        self.assertIn('ferryx_test_shard="3"', result['args'])
        self.assertFalse(any(a.startswith('-Z') for a in result['args']))

    def test_nonroot_or_nontest_invocations_are_untouched(self):
        for args in (['--crate-name','dependency','--test','--emit=link'], ['--crate-name','ferryx_lib','--emit=link']):
            for mode in ('shard', 'inventory'):
                with self.subTest(args=args, mode=mode):
                    result=self.wrapper(mode,args)
                    self.assertEqual(result['args'], ['/fake/rustc',*args])
                    self.assertIsNone(result['record'])
                    self.assertIsNone(result['bootstrap'])

    def test_contract_preserves_semantics_ignores_artifact_hashes(self):
        args=['--crate-name','ferryx_lib','--test','--target','x86_64-unknown-linux-gnu','--cfg','feature="desktop"','-C','debug-assertions=on','-C','metadata=old','--extern','serde=/old/lib.rlib']
        expected=runner.compile_contract(args)
        adjusted=[a.replace('metadata=old','metadata=new').replace('/old/','/new/') for a in args]
        adjusted += ['--cfg','ferryx_test_sharding','--cfg','ferryx_test_shard="2"']
        self.assertEqual(expected,runner.compile_contract(adjusted))
        for old,new in [('debug-assertions=on','debug-assertions=off'),('feature="desktop"','feature="other"'),('x86_64-unknown-linux-gnu','aarch64-apple-darwin')]:
            self.assertNotEqual(expected,runner.compile_contract([new if a==old else a for a in args]))

    def test_contract_split_joined_and_codegen_alias_are_equivalent(self):
        split = ['--test', '--crate-name', 'ferryx_lib', '--edition', '2021',
                 '--target', 'target', '--crate-type', 'lib', '--cfg', 'feature="desktop"',
                 '-C', 'opt-level=0', '--extern', 'serde=/old/lib.rlib', '-C', 'metadata=old']
        joined = ['--test', '--crate-name=ferryx_lib', '--edition=2021',
                  '--target=target', '--crate-type=lib', '--cfg=feature="desktop"',
                  '-Copt-level=0', '--extern=serde=/new/lib.rlib', '-Cmetadata=new']
        expected = runner.compile_contract(split)
        self.assertEqual(expected, runner.compile_contract(joined))
        for spelling in ('-C=opt-level=0', '--codegen=opt-level=0'):
            self.assertEqual(expected, runner.compile_contract([spelling if a == '-Copt-level=0' else a for a in joined]))
        self.assertEqual(expected, runner.compile_contract(joined + ['--cfg=ferryx_test_sharding', '--cfg=ferryx_test_shard="1"']))
        for old, new in [('-Copt-level=0', '-Copt-level=1'),
                         ('--cfg=feature="desktop"', '--cfg=feature="other"'),
                         ('--extern=serde=/new/lib.rlib', '--extern=other=/new/lib.rlib')]:
            self.assertNotEqual(expected, runner.compile_contract([new if a == old else a for a in joined]))

    def test_contract_rejects_missing_empty_and_conflicting_codegen(self):
        base = ['--crate-name', 'ferryx_lib', '--test']
        for tail in (['-C'], ['--cfg='], ['--extern', '--test'],
                     ['-Copt-level=0', '--codegen=opt-level=1']):
            with self.subTest(tail=tail), self.assertRaises(ValueError):
                runner.compile_contract(base + tail)
        # Only the two owned cfg names may be excluded, not similarly named inputs.
        self.assertNotEqual(runner.compile_contract(base),
                            runner.compile_contract(base + ['--cfg=ferryx_test_shard_other']))

    def prepare(self, root):
        source=root/'source'; source.mkdir()
        (source/'ui/dist').mkdir(parents=True); (source/'ui/dist/index.html').write_text('fixture')
        worktrees=root/'worktrees'; worktrees.mkdir()
        evidence=root/'evidence'; baseline=root/'baseline.json'
        state={'commit':'abc','tree':'def'}
        baseline.write_text(json.dumps({'snapshot':state,'tests':[{'name':'one','ignored':False}],'compile_contract':{},'source_locations':[]}))
        argv=['runner','--source-root',str(source),'--worktree-root',str(worktrees),'--evidence',str(evidence),'--target','target','--baseline',str(baseline)]
        return source,worktrees,evidence,state,argv

    def test_existing_evidence_is_rejected_before_any_process(self):
        with tempfile.TemporaryDirectory() as tmp:
            _,_,evidence,_,argv=self.prepare(Path(tmp)); evidence.mkdir()
            sentinel=evidence/'keep'; sentinel.write_text('owned elsewhere')
            with patch.object(sys,'argv',argv), patch.object(runner.subprocess,'run') as run:
                with self.assertRaises(FileExistsError): runner.main()
                run.assert_not_called()
            self.assertEqual(sentinel.read_text(),'owned elsewhere')

    def test_runtime_helper_failure_precedes_any_library_compilation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); _,_,evidence,state,argv=self.prepare(root)
            argv.append('--run-tests')
            def run(command, **kwargs):
                if command[:3] == ['git','worktree','add']:
                    Path(command[4]).mkdir()
                return subprocess.CompletedProcess(command,0)
            with patch.object(sys,'argv',argv), patch.object(runner,'snapshot',return_value=state), \
                 patch.object(runner,'reject_external_config'), patch.object(runner,'verify_runtime_fonts',return_value=[]), \
                 patch.object(runner,'verify_runtime_target'), patch.object(runner,'output',return_value=str(root/'.git')), \
                 patch.object(runner.subprocess,'run',side_effect=run), patch.object(runner.shutil,'copytree'), \
                 patch.object(runner,'prepare_runtime_helper',side_effect=RuntimeError('helper build failed')), \
                 patch.object(runner,'run_stage') as stage:
                with self.assertRaisesRegex(RuntimeError,'helper build failed'):
                    runner.main()
                stage.assert_not_called()
            self.assertEqual(json.loads((evidence/'result.json').read_text())['status'],'FAILED')

    def test_helper_preparation_is_runtime_only_and_before_inventory(self):
        for runtime in (False, True):
            with self.subTest(runtime=runtime), tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp); _,_,_,state,argv=self.prepare(root)
                argv=argv[:-2]  # Exercise new inventory, not cached baseline.
                if runtime: argv.append('--run-tests')
                calls=[]
                def run(command, **kwargs):
                    if command[:3] == ['git','worktree','add']:
                        Path(command[4]).mkdir()
                    return subprocess.CompletedProcess(command,0)
                def helper(*args):
                    calls.append('helper'); return {'sha256':'fresh'}
                def stage(label, *args):
                    calls.append(label); raise RuntimeError('inventory-stop')
                with patch.object(sys,'argv',argv), patch.object(runner,'snapshot',return_value=state), \
                     patch.object(runner,'reject_external_config'), patch.object(runner,'verify_runtime_fonts',return_value=[]), \
                     patch.object(runner,'verify_runtime_target'), patch.object(runner,'output',return_value=str(root/'.git')), \
                     patch.object(runner.subprocess,'run',side_effect=run), patch.object(runner.shutil,'copytree'), \
                     patch.object(runner,'prepare_runtime_helper',side_effect=helper), \
                     patch.object(runner,'run_stage',side_effect=stage):
                    with self.assertRaisesRegex(RuntimeError,'inventory-stop'):
                        runner.main()
                self.assertEqual(calls,['helper','inventory'] if runtime else ['inventory'])

    def test_failed_worktree_add_never_removes_existing_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); _,_,evidence,state,argv=self.prepare(root)
            calls=[]; collided=[]
            def run(command,**kwargs):
                calls.append(command)
                if command[:3]==['git','worktree','add']:
                    target=Path(command[4]); target.mkdir(); (target/'keep').write_text('foreign')
                    collided.append(target)
                    raise subprocess.CalledProcessError(128,command)
                return subprocess.CompletedProcess(command,0)
            with patch.object(sys,'argv',argv), patch.object(runner,'snapshot',return_value=state), patch.object(runner,'reject_external_config'), patch.object(runner,'output',return_value=str(root/'.git')), patch.object(runner.subprocess,'run',side_effect=run):
                with self.assertRaises(subprocess.CalledProcessError): runner.main()
            self.assertEqual((collided[0]/'keep').read_text(),'foreign')
            self.assertFalse(any(c[:3]==['git','worktree','remove'] for c in calls))
            self.assertEqual(json.loads((evidence/'result.json').read_text())['status'],'FAILED')


class FullRuntimeTests(unittest.TestCase):
    def test_all_binaries_run_unfiltered_and_totals_checked(self):
        import tempfile
        from pathlib import Path
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);log=root/'runtime.log'
            log.write_text('test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n')
            helper_binary=root/'helper'; helper_binary.write_bytes(b'fresh')
            state={'runtime_helper': {'staged': str(helper_binary), 'sha256': runner.sha(helper_binary)}, 'shards':[{'index':0,'executable':'binary','tests':[{'ignored':False},{'ignored':True}]}]}
            with patch.object(runner,'run_stage',return_value=log) as stage:
                runner.run_full_runtime(state,root,{},root,root/'bounded',30)
                self.assertEqual(stage.call_args.args[1],['binary','--test-threads=1'])
                self.assertEqual(stage.call_args.args[2], root/'src-tauri')
                self.assertEqual(state['runtime'],'FULL_RUNTIME_PASS')
            log.write_text('test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 1 filtered out; finished in 0.01s\n')
            with patch.object(runner,'run_stage',return_value=log), self.assertRaises(ValueError):
                runner.run_full_runtime(state,root,{},root,root/'bounded',30)
            self.assertEqual(state['runtime'],'INCOMPLETE')


class RuntimeFontTests(unittest.TestCase):
    def test_non_linux_does_not_require_linux_font_fixtures(self):
        with patch.object(runner.subprocess, 'check_output') as command:
            self.assertEqual(runner.verify_runtime_fonts('aarch64-apple-darwin', {}), [])
            command.assert_not_called()

    def test_exact_families_and_font_hashes_are_recorded(self):
        with tempfile.TemporaryDirectory() as directory:
            font = Path(directory)/'fixture.ttf'
            font.write_bytes(b'font-fixture')
            def match(argv, **kwargs):
                return argv[-1]+'\t'+str(font)+'\n'
            with patch.object(runner.subprocess, 'check_output', side_effect=match):
                records = runner.verify_runtime_fonts('x86_64-unknown-linux-gnu', {})
            self.assertEqual(len(records), 4)
            self.assertTrue(all(row['sha256'] == runner.sha(font) for row in records))

    def test_missing_mono_cannot_use_sans_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            font = Path(directory)/'fixture.ttf'
            font.write_bytes(b'font-fixture')
            def match(argv, **kwargs):
                family = 'DejaVu Sans' if argv[-1] == 'DejaVu Sans Mono' else argv[-1]
                return family+'\t'+str(font)+'\n'
            with patch.object(runner.subprocess, 'check_output', side_effect=match):
                with self.assertRaisesRegex(ValueError, 'DejaVu Sans Mono'):
                    runner.verify_runtime_fonts('x86_64-unknown-linux-gnu', {})

    def test_substitution_is_not_accepted_as_required_font(self):
        with patch.object(runner.subprocess, 'check_output', return_value='Other MesloLGS NF\t/fake.ttf\n'):
            with self.assertRaisesRegex(ValueError, 'required runtime font'):
                runner.verify_runtime_fonts('x86_64-unknown-linux-gnu', {})

    def test_missing_fontconfig_and_missing_font_file_fail(self):
        with patch.object(runner.subprocess, 'check_output', side_effect=FileNotFoundError):
            with self.assertRaisesRegex(ValueError, 'fontconfig'):
                runner.verify_runtime_fonts('x86_64-unknown-linux-gnu', {})
        with patch.object(runner.subprocess, 'check_output', return_value='MesloLGS NF\t/nonexistent-ferryx-font-fixture.ttf\n'):
            with self.assertRaisesRegex(ValueError, 'missing font file'):
                runner.verify_runtime_fonts('x86_64-unknown-linux-gnu', {})


class RuntimeHelperTests(unittest.TestCase):
    def fixture(self, directory, target='x86_64-unknown-linux-gnu'):
        root = Path(directory)
        generated = root/'unique-generated-checkout'
        (generated/'remote-helper').mkdir(parents=True)
        (generated/'remote-helper/Cargo.lock').write_text('locked helper dependencies')
        suffix = '.exe' if 'windows' in target else ''
        executable = root/'shared-cache'/target/'debug'/('ferryx-remote-helper'+suffix)
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b'fresh helper artifact'); executable.chmod(0o755)
        artifact = {'reason':'compiler-artifact', 'target':{'name':'ferryx-remote-helper','kind':['bin']},
                    'profile':{'test':False}, 'executable':str(executable)}
        log=root/'build.log'; log.write_text(json.dumps(artifact)+'\n')
        return root,generated,executable,artifact,log

    def test_target_aware_cargo_artifact_staged_at_fixture_path(self):
        for target in ('x86_64-unknown-linux-gnu', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc'):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as tmp:
                root,generated,executable,_,log=self.fixture(tmp,target)
                env={'CARGO_TARGET_DIR':str(root/'shared-cache')}
                with patch.object(runner,'run_stage',return_value=log) as stage:
                    record=runner.prepare_runtime_helper(target,generated,env,root,root/'bounded',45)
                self.assertEqual(stage.call_args.args, ('runtime-helper-build',
                    ['cargo','build','--manifest-path','remote-helper/Cargo.toml','--bin',
                     'ferryx-remote-helper','--target',target,'--locked','--offline','-j1','--message-format=json'],
                    generated,env,root,root/'bounded',45))
                staged=generated/'remote-helper/target/debug'/executable.name
                self.assertEqual(staged.read_bytes(), executable.read_bytes())
                self.assertEqual(staged.stat().st_mode & 0o777, 0o755)
                self.assertEqual(record['staged'],str(staged))
                self.assertEqual(record['sha256'],runner.sha(staged))
                self.assertEqual(record['lock_sha256'],runner.sha(generated/'remote-helper/Cargo.lock'))

    def test_missing_ambiguous_or_wrong_artifacts_fail_closed(self):
        for scenario in ('missing-file','ambiguous','test-harness','wrong-bin','no-artifact'):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as tmp:
                root,generated,executable,artifact,log=self.fixture(tmp)
                rows=[artifact]
                if scenario=='missing-file': executable.unlink()
                elif scenario=='ambiguous': rows.append(dict(artifact,executable=str(root/'other')))
                elif scenario=='test-harness': artifact['profile']['test']=True
                elif scenario=='wrong-bin': artifact['target']['name']='other'
                else: rows=[]
                log.write_text('\n'.join(map(json.dumps,rows)))
                with patch.object(runner,'run_stage',return_value=log), self.assertRaises(ValueError):
                    runner.prepare_runtime_helper('x86_64-unknown-linux-gnu',generated,{},root,root/'bounded',45)
                self.assertFalse((generated/'remote-helper/target/debug/ferryx-remote-helper').exists())

    def test_existing_staged_binary_is_replaced_by_fresh_artifact(self):
        with tempfile.TemporaryDirectory() as tmp:
            root,generated,executable,_,log=self.fixture(tmp)
            staged=generated/'remote-helper/target/debug/ferryx-remote-helper'
            staged.parent.mkdir(parents=True); staged.write_bytes(b'stale')
            with patch.object(runner,'run_stage',return_value=log):
                runner.prepare_runtime_helper('x86_64-unknown-linux-gnu',generated,{},root,root/'bounded',45)
            self.assertEqual(staged.read_bytes(), executable.read_bytes())

    def test_missing_or_changed_helper_prevents_any_test_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            helper=Path(tmp)/'helper'; helper.write_bytes(b'new')
            for record in (None, {'staged':str(helper),'sha256':'stale'}):
                state={'runtime_helper':record,'shards':[{'index':0,'executable':'must-not-run'}]}
                with patch.object(runner,'run_stage') as stage:
                    with self.assertRaisesRegex(ValueError,'helper is missing or changed'):
                        runner.run_full_runtime(state,Path('generated'),{},Path('evidence'),Path('bounded'),30)
                    stage.assert_not_called()
                self.assertEqual(state['runtime'],'INCOMPLETE')

    def test_runtime_target_must_match_rustc_host(self):
        runner.verify_runtime_target({'rustc':'rustc 1.99.1\nhost: x86_64-unknown-linux-gnu\n',
                                      'target':'x86_64-unknown-linux-gnu'})
        for compiler in ('rustc 1.99.1', 'host: aarch64-apple-darwin\n'):
            with self.assertRaisesRegex(ValueError, 'native rustc host'):
                runner.verify_runtime_target({'rustc':compiler,'target':'x86_64-unknown-linux-gnu'})


if __name__ == "__main__":
    unittest.main()
