"""Socket-free parser checks; fixture uses actual rustc-generated MIR shape."""
import unittest
from test_shard_inventory import (parse_mir, parse_compiled_list,
                                 compare_compiled_lists, verify_disjoint_union, parse_expanded)

MIR = '''const test: TestDescAndFn = {
    _2 = StaticTestName(const "nested::generated");
    _1 = TestDesc { name: move _2, ignore: const true, ignore_message: move _3 };
}
fn main() -> () {
    _0 = test_main_static(move _1) -> [return: bb1, unwind continue];
}
const main::promoted[0]: &[&TestDescAndFn; 1] = {
    _4 = const nested::generated;
}
'''
ROWS = [{'name':'nested::generated','ignored':True}]
LIST = 'nested::generated: test\n\n1 test, 0 benchmarks\n'

class InventoryTests(unittest.TestCase):
    def test_descriptor_name_not_short_const_header(self):
        self.assertEqual(parse_mir(MIR),ROWS)
    def test_malformed_mir_rejected(self):
        for bad in [MIR.replace('StaticTestName','DynamicTestName'),
                    MIR.replace('name: move _2','name: move _999'),
                    MIR.replace('&[&TestDescAndFn; 1] =','&[&TestDescAndFn; 2] ='),
                    MIR.replace('test_main_static','unknown_harness'),
                    MIR.replace('_4 = const nested::generated;',''),
                    MIR + MIR]:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                parse_mir(bad)
    def test_list(self):
        self.assertEqual(parse_compiled_list(LIST),['nested::generated'])
        self.assertEqual(parse_compiled_list('0 tests, 0 benchmarks\n'),[])
    def test_bad_lists(self):
        for bad in ['', 'error\n'+LIST, LIST.replace('1 test','2 tests'),
                    'x: benchmark\n\n0 tests, 1 benchmark\n',
                    'x: test\nx: test\n\n2 tests, 0 benchmarks\n']:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                parse_compiled_list(bad)
    def test_compiled_matches_ignored(self):
        compare_compiled_lists(ROWS,LIST,LIST)
        with self.assertRaises(ValueError):
            compare_compiled_lists(ROWS,LIST,'0 tests, 0 benchmarks\n')
    def test_disjoint_union(self):
        other={'name':'plain','ignored':False}
        verify_disjoint_union(ROWS+[other],[ROWS,[other]])
        for shards in [[ROWS,ROWS],[],[[dict(ROWS[0],ignored=False)]]]:
            with self.subTest(shards=shards), self.assertRaises(ValueError):
                verify_disjoint_union(ROWS,shards)

EXPANDED = """#[rustc_test_marker = "nested::generated"]
#[doc(hidden)]
pub const generated: test::TestDescAndFn =
 test::TestDescAndFn {
  desc: test::TestDesc {
   name: test::StaticTestName("nested::generated"),
   ignore: true,
  },
 };
#[rustc_main]
#[coverage(off)]
#[doc(hidden)]
pub fn main() -> () {
 extern crate test;
 test::test_main_static(&[&generated])
}
"""

class ExpandedInventoryTests(unittest.TestCase):
    def test_expanded(self):
        self.assertEqual(parse_expanded(EXPANDED),ROWS)
    def test_compiler_wrapped_long_descriptor_tokens(self):
        # Actual full-crate rustc prettyprinter wraps at these boundaries.
        wrapped=(EXPANDED.replace('rustc_test_marker = ', 'rustc_test_marker =\n    ')
                 .replace('pub const generated:', 'pub const\n    generated:')
                 .replace(': test::TestDescAndFn =', ':\n    test::TestDescAndFn\n    ='))
        self.assertEqual(parse_expanded(wrapped),ROWS)

    def test_repeated_short_identifiers_counted(self):
        descriptor=EXPANDED.split('#[rustc_main]')[0]
        extra=descriptor.replace('nested::generated','other::generated')
        text=(extra+EXPANDED).replace('&[&generated]', '&[&generated, &generated]')
        self.assertEqual([r['name'] for r in parse_expanded(text)],
                         ['nested::generated','other::generated'])

    def test_rejects_unknown_or_partial_shapes(self):
        for bad in [EXPANDED.replace('ignore: true','ignore: expression'),
                    EXPANDED.replace('StaticTestName("nested::generated")','StaticTestName("wrong")'),
                    EXPANDED.replace('&generated','&other'),
                    EXPANDED.replace('&generated','&generated, &generated'),
                    EXPANDED.replace('#[rustc_main]',''),
                    EXPANDED[:-3], EXPANDED+"unexpected suffix"]:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                parse_expanded(bad)



class SourceCoverageTests(unittest.TestCase):
    def test_shared_path_coverage_and_missing_rejected(self):
        import tempfile
        from pathlib import Path
        from test_shard_inventory import verify_source_coverage
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            path='src-tauri/tests/support/shared.rs'
            file=root/path;file.parent.mkdir(parents=True)
            file.write_text('#[ignore]\n#[test]\nfn shared() {}\n')
            manifest={'files':[{'path':path,'tests':[{'fn_offset':18}]}]}
            rows=[{'name':'nested::shared','source_file':'src/remote/../../tests/support/shared.rs','line':3}]
            verify_source_coverage(rows,manifest,root)
            with self.assertRaises(ValueError):verify_source_coverage(rows,{'files':[]},root)
            with self.assertRaises(ValueError):verify_source_coverage(rows,{'files':manifest['files']*2},root)
            with self.assertRaises(ValueError):verify_source_coverage([dict(rows[0],source_file='../../outside.rs')],manifest,root)

class SelfDispatchTests(unittest.TestCase):
    def fixture(self, sources, plans=None):
        import tempfile
        from pathlib import Path
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        for path, source in sources.items():
            file = root / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(source)
        manifest = {'files': plans or [
            {'path': path, 'cohort': 'shared', 'tests': [{'bucket': 0}]}
            for path in sources]}
        return root, manifest

    def verify(self, root, manifest, names=('module::child',)):
        from test_shard_inventory import verify_self_dispatch_cohorts
        return verify_self_dispatch_cohorts([
            {'name': name, 'source_file': 'src/child.rs', 'line': 1}
            for name in names], manifest, root)

    def test_helper_only_cross_file_edge_and_ignored_target(self):
        root, plan = self.fixture({
            'src-tauri/src/helper.rs': 'fn launch() { Command::new(current_exe()).args(["--exact", "module::child", "--ignored"]); }',
            'src-tauri/src/child.rs': '#[ignore] #[test] fn child() {}'})
        plan['files'][0]['tests'] = []
        edges = self.verify(root, plan)
        self.assertEqual(edges[0]['target'], 'module::child')
        self.assertEqual(edges[0]['source_file'], 'src-tauri/src/helper.rs')
        plan['files'][1]['cohort'] = 'other'
        with self.assertRaisesRegex(ValueError, 'cross-file cohort'):
            self.verify(root, plan)

    def test_unplanned_helper_is_not_silently_ignored(self):
        root, plan = self.fixture({
            'src-tauri/src/helper.rs': 'fn launch() { Command::new(current_exe()).args(["--exact", "module::child"]); }',
            'src-tauri/src/child.rs': '#[test] fn child() {}'})
        plan['files'] = plan['files'][1:]
        with self.assertRaisesRegex(ValueError, 'cross-file cohort'):
            self.verify(root, plan)

    def test_unique_short_filter_and_constants(self):
        for args in ['["child", "--nocapture"]', '["--exact", TARGET]']:
            root, plan = self.fixture({
                'src-tauri/src/child.rs': 'const TARGET: &str = "module::child"; fn run() { Command::new(current_exe()).args(' + args + '); }'})
            self.assertEqual(self.verify(root, plan)[0]['target'], 'module::child')
        with self.assertRaisesRegex(ValueError, 'ambiguous'):
            (root / 'src-tauri/src/child.rs').write_text('fn run() { Command::new(current_exe()).args(["child", "--nocapture"]); }')
            self.verify(root, plan, ('module::child', 'other::child'))

    def test_missing_target_and_split_bucket_rejected(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': 'fn run() { Command::new(current_exe()).args(["--exact", "missing"]); }'})
        with self.assertRaisesRegex(ValueError, 'missing or ambiguous'):
            self.verify(root, plan)
        (root / 'src-tauri/src/child.rs').write_text('fn run() { Command::new(current_exe()).args(["--exact", "module::child"]); }')
        plan['files'][0]['tests'].append({'bucket': 1})
        with self.assertRaisesRegex(ValueError, 'split'):
            self.verify(root, plan)

    def test_dynamic_selector_requires_pinned_review(self):
        import hashlib
        source = 'fn run(name: &str) { Command::new(current_exe()).args(["--exact", name]); }'
        root, plan = self.fixture({'src-tauri/src/child.rs': source})
        with self.assertRaisesRegex(ValueError, 'SHA-pinned'):
            self.verify(root, plan)
        plan['self_dispatch_reviews'] = {'src-tauri/src/child.rs': {
            'source_sha256': hashlib.sha256(source.encode()).hexdigest(),
            'reason': 'All callers audited; only module::child is passed.',
            'targets': ['module::child']}}
        self.assertEqual(self.verify(root, plan)[0]['target'], 'module::child')
        (root / 'src-tauri/src/child.rs').write_text(source + '\n')
        with self.assertRaisesRegex(ValueError, 'SHA-pinned'):
            self.verify(root, plan)

    def test_dynamic_argument_vector_fails_closed(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': 'fn run() { Command::new(current_exe()).args(targets()); }'})
        with self.assertRaisesRegex(ValueError, 'SHA-pinned'):
            self.verify(root, plan)

    def test_path_only_current_exe_and_quoted_fake_calls_are_not_dispatch(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': '''
        fn path() { let path = current_exe().unwrap(); assert!(path.exists()); }
        // current_exe().args(["--exact", "missing"]);
        /* /* nested */ current_exe().arg("--exact").arg("missing"); */
        const SCRIPT: &str = r###"Command::new(current_exe()).args(["--exact", "missing"])"###;
        '''})
        self.assertEqual(self.verify(root, plan), [])

    def test_individual_argument_chain_is_checked(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': 'fn run() { Command::new(current_exe()).arg("--exact").arg("module::child"); }'})
        self.assertEqual(self.verify(root, plan)[0]['target'], 'module::child')

    def test_distinct_arg_chains_do_not_share_harness_flags(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': '''
            fn run() {
                Command::new(current_exe()).arg("--exact").arg("module::child");
                Command::new("git").arg("status");
            }
        '''})
        self.assertEqual(len(self.verify(root, plan)), 1)

    def test_inactive_test_body_uses_compiler_inventory_not_guessed_cfg(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': '''
            #[test] fn inactive() { Command::new(current_exe()).args(["--exact", "missing"]); }
            #[test] fn child() {}
        '''})
        plan['files'][0]['tests'] += [{'function': 'inactive', 'bucket': 0}]
        self.assertEqual(self.verify(root, plan), [])

    def test_non_dispatch_review_and_inactive_selector_are_pinned(self):
        import hashlib
        source = 'fn helper() { Command::new(current_exe()).args(["--exact", "windows::helper"]); }'
        root, plan = self.fixture({'src-tauri/src/child.rs': source})
        plan['self_dispatch_reviews'] = {'src-tauri/src/child.rs': {
            'source_sha256': hashlib.sha256(source.encode()).hexdigest(),
            'reason': 'Owning module is cfg(windows), absent from Linux compilation.',
            'targets': [], 'inactive_selectors': ['windows::helper']}}
        self.assertEqual(self.verify(root, plan), [])
        with self.assertRaisesRegex(ValueError, 'active excluded'):
            self.verify(root, plan, ('windows::helper',))


    def test_short_filter_rejects_substring_collision(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': 'fn run() { Command::new(current_exe()).args(["child", "--nocapture"]); }'})
        with self.assertRaisesRegex(ValueError, 'ambiguous'):
            self.verify(root, plan, ('module::child', 'module::other_child'))

    def test_unfiltered_harness_dispatch_requires_review(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': 'fn run() { Command::new(current_exe()).args(["--exact"]); }'})
        with self.assertRaisesRegex(ValueError, 'SHA-pinned'):
            self.verify(root, plan)

    def test_path_metadata_does_not_taint_unrelated_dynamic_process_args(self):
        root, plan = self.fixture({'src-tauri/src/child.rs': 'fn run() { let path = current_exe().unwrap(); assert!(path.exists()); Command::new("git").args(options()); }'})
        self.assertEqual(self.verify(root, plan), [])

if __name__=='__main__': unittest.main()
