"""Version-sensitive, fail-closed inventory prototype; not a stable Rust API."""
import collections,json,re,sys
from pathlib import Path

def parse_mir(text):
    blocks=re.findall(r'^const [^\n]+: TestDescAndFn = \{\n(.*?)^\}',text,re.M|re.S)
    if not blocks: raise ValueError('no test descriptors found')
    rows=[]
    for block in blocks:
        names=re.findall(r'^\s*(_\d+) = StaticTestName\(const ("(?:[^"\\]|\\.)*")\);$',block,re.M)
        desc=re.findall(r'^\s*_\d+ = TestDesc \{ name: move (_\d+), ignore: const (true|false),',block,re.M)
        if len(names)!=1 or len(desc)!=1 or names[0][0]!=desc[0][0]: raise ValueError('unrecognized test name/descriptor shape')
        name=json.loads(names[0][1]) # unsupported Rust-only escapes deliberately rejected
        if not name or '\n' in name: raise ValueError('invalid test name')
        rows.append({'name':name,'ignored':desc[0][1]=='true'})
    if len({r['name'] for r in rows})!=len(rows): raise ValueError('duplicate test names')
    harness=re.findall(r'^const main::promoted\[\d+\]: &\[&TestDescAndFn; (\d+)\] = \{\n(.*?)^\}',text,re.M|re.S)
    if len(harness)!=1: raise ValueError('unrecognized harness array')
    count,body=harness[0]
    refs=re.findall(r'^\s*_\d+ = const ([A-Za-z_][\w:]*)\s*;$',body,re.M)
    if int(count)!=len(rows) or len(refs)!=len(rows): raise ValueError('descriptor/harness count mismatch')
    if text.count('= test_main_static(')!=1: raise ValueError('missing or ambiguous test harness call')
    return sorted(rows,key=lambda r:r['name'])

def parse_compiled_list(text):
    """Parse standard `libtest --list` output; reject diagnostics/benchmarks."""
    lines=[line for line in text.splitlines() if line]
    if not lines:
        raise ValueError('missing libtest summary')
    summary=re.fullmatch(r'(\d+) tests?, (\d+) benchmarks?',lines[-1])
    if summary is None or int(summary[2]) != 0:
        raise ValueError('unknown summary or unexpected benchmarks')
    names=[]
    for line in lines[:-1]:
        if not line.endswith(': test') or not line[:-6]:
            raise ValueError('unexpected libtest output')
        names.append(line[:-6])
    if len(names)!=int(summary[1]) or len(set(names))!=len(names):
        raise ValueError('count mismatch or duplicate listed test')
    return sorted(names)


def compare_compiled_lists(inventory, all_text, ignored_text):
    """Require exact compiled all/ignored names, preserving ignored metadata."""
    expected={row['name'] for row in inventory}
    if len(expected)!=len(inventory):
        raise ValueError('duplicate inventory name')
    ignored={row['name'] for row in inventory if row['ignored']}
    actual=set(parse_compiled_list(all_text))
    actual_ignored=set(parse_compiled_list(ignored_text))
    if actual!=expected or actual_ignored!=ignored:
        raise ValueError('compiled inventory or ignored metadata mismatch')


def verify_disjoint_union(full_inventory, shard_inventories):
    """Every compiled descriptor occurs exactly once, with identical metadata."""
    def mapping(rows):
        result={row['name']:row for row in rows}
        if len(result)!=len(rows):
            raise ValueError('duplicate inventory name')
        return result
    expected=mapping(full_inventory)
    actual={}
    for shard in shard_inventories:
        part=mapping(shard)
        if actual.keys() & part.keys():
            raise ValueError('shard inventories overlap')
        actual.update(part)
    if actual!=expected:
        raise ValueError('shard union differs from full inventory')



def parse_expanded(text):
    string=r'"(?:[^"\\]|\\.)*"'
    pattern=(r'#\[rustc_test_marker\s*=\s*('+string+r')\]\s*'
             r'#\[doc\(hidden\)\]\s*pub const\s+(\w+):\s*test::TestDescAndFn\s*=\s*'
             r'test::TestDescAndFn \{\s*desc: test::TestDesc \{\s*'
             r'name: test::StaticTestName\(('+string+r')\),\s*ignore: (true|false),')
    matches=re.findall(pattern,text)
    if not matches or len(matches)!=len(re.findall(r'#\[rustc_test_marker\s*=',text)):
        raise ValueError('missing or unrecognized generated test descriptor')
    rows=[]; identifiers=[]
    for marker,ident,name,ignored in matches:
        name=json.loads(name)
        if json.loads(marker)!=name or not name or '\n' in name:
            raise ValueError('test marker/name mismatch')
        rows.append({'name':name,'ignored':ignored=='true'})
        identifiers.append(ident)
    if len({r['name'] for r in rows})!=len(rows):
        raise ValueError('duplicate generated test name')
    main=re.findall(r'#\[rustc_main\]\s*#\[coverage\(off\)\]\s*#\[doc\(hidden\)\]\s*pub fn main\(\) -> \(\) \{\s*extern crate test;\s*test::test_main_static\(&\[(.*?)\]\)\s*\}\s*\Z',text,re.S)
    if len(main)!=1:
        raise ValueError('missing or unrecognized generated main')
    references=[r.strip() for r in main[0].split(',') if r.strip()]
    if any(re.fullmatch(r'&\w+',r) is None for r in references):
        raise ValueError('unknown test reference shape')
    if collections.Counter(r[1:] for r in references)!=collections.Counter(identifiers):
        raise ValueError('generated descriptor/main reference mismatch')
    return sorted(rows,key=lambda r:r['name'])

if __name__=='__main__':
    print(json.dumps(parse_mir(Path(sys.argv[1]).read_text()),indent=2,ensure_ascii=False))


def parse_expanded_locations(text):
    """Extract source locations for every already-validated compiler descriptor."""
    inventory = parse_expanded(text)
    blocks = re.findall(r'#\[rustc_test_marker\s*=\s*("(?:[^"\\]|\\.)*")\](.*?)(?=#\[rustc_test_marker|\Z)', text, re.S)
    rows = []
    for name, block in blocks:
        match = re.search(r'source_file:\s*("(?:[^"\\]|\\.)*"),\s*start_line:\s*(\d+)usize', block)
        if match is None:
            raise ValueError('missing compiler descriptor source location')
        rows.append({'name': json.loads(name), 'source_file': json.loads(match[1]), 'line': int(match[2])})
    if {row['name'] for row in rows} != {row['name'] for row in inventory}:
        raise ValueError('source location inventory differs')
    return rows


def verify_source_coverage(locations, manifest, root):
    """Every compiler test must map to exactly one AST-instrumented function."""
    root = Path(root).resolve()
    planned = collections.Counter()
    for file in manifest['files']:
        path = root / file['path']
        raw = path.read_bytes()
        for test in file['tests']:
            planned[(path.resolve(), raw[:test['fn_offset']].count(b'\n') + 1)] += 1
    for row in locations:
        path = (root / 'src-tauri' / row['source_file']).resolve()
        if not path.is_relative_to(root) or planned[(path, row['line'])] != 1:
            raise ValueError('compiler test has no unique AST instrumentation: ' + row['name'])


def verify_self_dispatch_cohorts(locations, manifest, root):
    """Conservative source preflight, not a general Rust data-flow proof.

    Inspect source files without tests as well as instrumented files. Supported
    selectors are literal `.arg`/`.args` values and local string constants. A
    dynamic selector needs an explicitly reviewed, SHA-pinned manifest entry:
    self_dispatch_reviews[path] = {source_sha256, reason, targets: [full names]}.
    This mapping is intentionally invalidated by *any* source edit.
    """
    import hashlib
    root = Path(root).resolve()
    plans = {file['path']: file for file in manifest['files']}
    if len(plans) != len(manifest['files']):
        raise ValueError('duplicate source plan')
    by_name, by_short = {}, collections.defaultdict(list)
    for row in locations:
        path = (root / 'src-tauri' / row['source_file']).resolve()
        if not path.is_relative_to(root) or row['name'] in by_name:
            raise ValueError('invalid self-dispatch descriptor location')
        by_name[row['name']] = path.relative_to(root).as_posix()
        by_short[row['name'].split('::')[-1]].append(row['name'])

    # Keep strings as opaque tokens: comments, quoted fixture programs and raw
    # strings must not introduce fake Rust calls. Nested block comments are legal.
    def tokens(source):
        result, i = [], 0
        raw_re = re.compile(r'(?:b)?r(#+)?"')
        quoted_re = re.compile(r'(?:b)?"(?:[^"\\]|\\.)*"', re.S)
        char_re = re.compile(r"'(?:[^'\\\n]|\\(?:.|u\{[^}]*\}))'")
        word_re = re.compile(r'[A-Za-z_][A-Za-z_0-9]*')
        while i < len(source):
            if source[i].isspace(): i += 1; continue
            if source.startswith('//', i):
                end = source.find('\n', i); i = len(source) if end < 0 else end; continue
            if source.startswith('/*', i):
                depth, i = 1, i + 2
                while depth and i < len(source):
                    if source.startswith('/*', i): depth += 1; i += 2
                    elif source.startswith('*/', i): depth -= 1; i += 2
                    else: i += 1
                if depth: raise ValueError('unterminated source comment')
                continue
            raw = raw_re.match(source, i)
            if raw:
                start = raw.end(); endmark = '"' + (raw[1] or '')
                end = source.find(endmark, start)
                if end < 0: raise ValueError('unterminated raw string')
                result.append(('string', source[start:end])); i = end + len(endmark); continue
            quoted = quoted_re.match(source, i)
            if quoted:
                text = quoted[0].removeprefix('b')
                try: value = json.loads(text)
                except ValueError: value = None  # not a supported test-name literal
                result.append(('string', value)); i = quoted.end(); continue
            # Char literals, unlike lifetimes, have a closing quote.
            char = char_re.match(source, i)
            if char: result.append(('char', char[0])); i = char.end(); continue
            word = word_re.match(source, i)
            if word: result.append(('word', word[0])); i = word.end(); continue
            result.append(('punct', source[i])); i += 1
        return result

    def close(ts, start):
        pairs = {'(': ')', '[': ']', '{': '}'}
        stack = [pairs[ts[start][1]]]
        for i in range(start + 1, len(ts)):
            value = ts[i][1]
            if ts[i][0] != 'punct': continue
            if value in pairs: stack.append(pairs[value])
            elif value in pairs.values():
                if not stack or value != stack.pop(): raise ValueError('unbalanced source tokens')
                if not stack: return i
        raise ValueError('unterminated argument expression')

    sources = {root / path for path in plans}
    for directory in (root / 'src-tauri/src', root / 'src-tauri/tests/support'):
        if directory.exists(): sources.update(directory.rglob('*.rs'))
    reviews = manifest.get('self_dispatch_reviews', {})
    used_reviews, edges = set(), []
    for source_path in sorted(sources):
        if not source_path.resolve().is_relative_to(root):
            raise ValueError('self-dispatch source escaped root')
        path = source_path.relative_to(root).as_posix()
        source = source_path.read_text()
        if not any(marker in source for marker in ('current_exe', 'own_executable', '--exact', '/proc/self/exe')):
            continue
        ts = tokens(source)
        # The compiler inventory, not a guessed cfg evaluator, identifies active
        # test functions. Do not validate dispatch inside cfg-disabled tests.
        planned_functions = {test['function'] for test in plans.get(path, {}).get('tests', [])
                             if 'function' in test}
        active_functions = {name.split('::')[-1] for name, file in by_name.items() if file == path}
        inactive = planned_functions - active_functions
        filtered, i = [], 0
        while i < len(ts):
            if ts[i] == ('word', 'fn') and i + 1 < len(ts) and ts[i + 1][1] in inactive:
                opening = next((j for j in range(i + 2, len(ts)) if ts[j] == ('punct', '{')), None)
                if opening is None: raise ValueError('unsupported inactive test shape: ' + path)
                i = close(ts, opening) + 1
            else:
                filtered.append(ts[i]); i += 1
        ts = filtered
        words = {value for kind, value in ts if kind == 'word'}
        strings = {value for kind, value in ts if kind == 'string'}
        if not ({'current_exe', 'own_executable'} & words or '--exact' in strings or '/proc/self/exe' in strings):
            continue
        constants = {}
        for i, token in enumerate(ts):
            if token == ('word', 'const') and i + 1 < len(ts):
                end = next((j for j in range(i + 2, len(ts)) if ts[j] == ('punct', ';')), len(ts))
                assignment = [j for j in range(i + 2, end) if ts[j] == ('punct', '=')]
                if len(assignment) == 1 and ts[assignment[0] + 1:end] and len(ts[assignment[0] + 1:end]) == 1:
                    value = ts[assignment[0] + 1]
                    if value[0] == 'string': constants[ts[i + 1][1]] = value[1]
        candidates, dynamic = set(), []
        review = reviews.get(path, {})
        pinned_review = (review.get('source_sha256') == hashlib.sha256(source_path.read_bytes()).hexdigest()
                         and bool(review.get('reason')) and isinstance(review.get('targets'), list))
        excluded_used = set()

        # Single arg chains are combined for a source-level conservative check.
        groups, singles = [], collections.defaultdict(list)
        # Track simple lexical aliases of this image. Unknown selectors on a
        # known self-image command fail closed, while unrelated git/ssh command
        # argument expressions in the same source file are not test dispatch.
        function_ranges = []
        for index in range(len(ts) - 2):
            if ts[index] != ('word', 'fn'): continue
            opening = next((j for j in range(index + 2, len(ts)) if ts[j] in (('punct', '{'), ('punct', ';'))), None)
            if opening is not None and ts[opening] == ('punct', '{'):
                function_ranges.append((opening, close(ts, opening)))
        def self_command(index):
            scope = max((start for start, end in function_ranges if start < index < end), default=0)
            aliases = {'current_exe', 'own_executable'}
            # Lexical declaration order supports arbitrary-length alias chains.
            for position in range(scope, index):
                if ts[position] != ('word', 'let') or position + 2 >= index: continue
                name_index = position + 1 + (ts[position + 1] == ('word', 'mut'))
                if ts[name_index][0] != 'word': continue
                end = next((j for j in range(name_index + 1, index) if ts[j] == ('punct', ';')), index)
                expression = ts[name_index + 1:end]
                if any(kind == 'word' and value in aliases for kind, value in expression):
                    aliases.add(ts[name_index][1])
                else:
                    aliases.discard(ts[name_index][1])
            statement = next((j for j in range(index - 1, scope, -1) if ts[j] == ('punct', ';')), scope)
            return any(kind == 'word' and value in aliases for kind, value in ts[statement + 1:index])
        for i in range(len(ts) - 3):
            if ts[i] != ('punct', '.') or ts[i + 1][1] not in ('arg', 'args') or ts[i + 2] != ('punct', '('): continue
            end = close(ts, i + 2); body = ts[i + 3:end]
            if ts[i + 1][1] == 'arg':
                statement = next((j for j in range(i - 1, -1, -1) if ts[j] == ('punct', ';')), -1)
                singles[statement].append((body, self_command(i)))
                continue
            if body and body[0] == ('punct', '&'): body = body[1:]
            if body and body[0] == ('punct', '[') and close(body, 0) == len(body) - 1:
                parts, part, depth = [], [], 0
                for token in body[1:-1] + [('punct', ',')]:
                    if token == ('punct', ',') and depth == 0:
                        if part: parts.append(part)
                        part = []; continue
                    part.append(token)
                    if token[0] == 'punct':
                        if token[1] in '([{': depth += 1
                        elif token[1] in ')]}': depth -= 1
                groups.append((parts, self_command(i)))
            elif self_command(i):
                dynamic.append('nonliteral .args expression')
        groups.extend(([body for body, _ in chain], any(is_self for _, is_self in chain)) for chain in singles.values())
        for group, possible_self in groups:
            values = [part[0][1] if len(part) == 1 and part[0][0] == 'string'
                      else constants.get(part[0][1]) if len(part) == 1 and part[0][0] == 'word'
                      else None for part in group]
            harness = any(v in ('--exact', '--ignored', '--nocapture', '--test-threads=1') for v in values)
            known = any(v in by_name or v in by_short for v in values if v is not None)
            if not harness and not known:
                if any(value is None for value in values) and possible_self:
                    dynamic.append('unresolved argument on possible self-image command')
                continue
            selected = False
            for value in values:
                if value is None: dynamic.append('dynamic test selector'); continue
                if value.startswith('-') or value.isdigit(): continue
                targets = ([value] if value in by_name else []) if '--exact' in values else [name for name in by_name if value in name]
                selected = True
                if not targets and pinned_review and value in review.get('inactive_selectors', []):
                    excluded_used.add(value)
                    used_reviews.add(path)
                    continue
                if len(targets) != 1:
                    raise ValueError('missing or ambiguous self-test selector: ' + path + ': ' + value)
                candidates.add((value, targets[0]))
            if harness and not selected:
                dynamic.append('unfiltered or dynamic harness dispatch')
        if set(review.get('inactive_selectors', [])) != excluded_used:
            raise ValueError('unused or active excluded selector: ' + path)
        if dynamic:
            if not pinned_review:
                raise ValueError('unsupported self-dispatch requires SHA-pinned review: ' + path + ': ' + ', '.join(sorted(set(dynamic))))
            used_reviews.add(path)
            for target in review['targets']:
                if target not in by_name: raise ValueError('reviewed target absent from compiler inventory: ' + target)
                candidates.add(('<reviewed dynamic>', target))
        for literal, target in sorted(candidates):
            file, target_file = plans.get(path), plans.get(by_name[target])
            if not file or not file.get('cohort') or not target_file or target_file.get('cohort') != file['cohort']:
                raise ValueError('self-test target requires an explicit cross-file cohort: ' + path + ' -> ' + target)
            cohort_tests = [test for plan in plans.values() if plan.get('cohort') == file['cohort'] for test in plan['tests']]
            if not cohort_tests or len({test['bucket'] for test in cohort_tests}) != 1:
                raise ValueError('self-test cohort was split: ' + file['cohort'])
            edges.append({'source_file': path, 'literal': literal, 'target': target, 'cohort': file['cohort']})
    if set(reviews) != used_reviews:
        raise ValueError('unused self-dispatch review; remove stale mapping')
    return edges
