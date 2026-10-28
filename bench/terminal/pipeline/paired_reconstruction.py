#!/usr/bin/env python3
"""Paired subprocess measurements of the real isolated Ghostty reconstruction phase.

No build, app, socket, PTY or network operation. Run only with audited pipeline
binaries and no concurrent compiler/benchmark. An A/A run passes the same binary
as both inputs. Bootstrap units are process pairs, not correlated inner samples.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import statistics
import subprocess
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def summarize(pairs, seed):
    logs = [math.log(p['head_median_ms'] / p['baseline_median_ms']) for p in pairs]
    rng = random.Random(seed)
    bootstrap = sorted(math.exp(statistics.mean(rng.choices(logs, k=len(logs))))
                       for _ in range(20000))
    return {
        'process_pairs': len(pairs),
        'head_over_baseline_geomean_ratio': math.exp(statistics.mean(logs)),
        'paired_bootstrap_95pct_ratio': [bootstrap[500], bootstrap[19499]],
        'pair_ratio_min_max': [math.exp(min(logs)), math.exp(max(logs))],
        'baseline_process_median_range_ms': [min(p['baseline_median_ms'] for p in pairs),
                                             max(p['baseline_median_ms'] for p in pairs)],
        'head_process_median_range_ms': [min(p['head_median_ms'] for p in pairs),
                                         max(p['head_median_ms'] for p in pairs)],
        'unit': 'per reconstruction milliseconds; >1 ratio means head slower',
        'uncertainty_scope': 'paired process-block resampling, conditional on this machine/profile/order; not whole-app performance',
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--head', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--mode', choices=['reconstruct64', 'reconstruct8m'], default='reconstruct64')
    parser.add_argument('--pairs', type=int, default=10)
    parser.add_argument('--seed', type=int, default=20261027)
    args = parser.parse_args()
    if args.pairs < 4 or args.pairs % 2:
        parser.error('pairs must be an even number >=4 for balanced order')
    binaries = {'baseline': args.baseline.resolve(strict=True), 'head': args.head.resolve(strict=True)}
    hashes = {label: digest(path) for label, path in binaries.items()}
    args.output.mkdir(parents=True, exist_ok=False)
    order = [('baseline', 'head')] * (args.pairs // 2) + [('head', 'baseline')] * (args.pairs // 2)
    random.Random(args.seed).shuffle(order)
    result = {'seed': args.seed, 'order': order, 'binary_sha256': hashes,
              'mode': args.mode,
              'binaries': {key: str(value) for key, value in binaries.items()},
              'pairs': [], 'status': 'INCOMPLETE', 'scope': 'real Ghostty reconstruction only, no UI/PTY/GPU'}
    path = args.output / 'result.json'
    fixture = None
    large = args.mode == 'reconstruct8m'
    fixture_name = 'original_8388608_reconstruction_isolated' if large else 'original_65536_reconstruction'
    phase = 'ghostty_reconstruct_8388608_isolated_batched' if large else 'ghostty_reconstruct_65536_batched'
    size, groups, count, batch = (8388612, 399457, 10, 5) if large else (65535, 3120, 30, 100)
    for pair, labels in enumerate(order):
        row = {'pair': pair, 'order': labels}
        for label in labels:
            assert digest(binaries[label]) == hashes[label], 'binary changed during measurement'
            raw = args.output / f'{pair:02d}-{label}.jsonl'
            started = time.monotonic()
            with raw.open('w') as output, raw.with_suffix('.stderr').open('w') as stderr:
                subprocess.run([str(binaries[label]), args.mode], stdout=output,
                               stderr=stderr, check=True, timeout=120)
            records = [json.loads(line) for line in raw.read_text().splitlines()]
            fixtures = [x for x in records if x.get('fixture') == fixture_name]
            assert len(fixtures) == 1
            if fixture is None:
                fixture = fixtures[0]
            assert fixtures[0] == fixture, 'fixture or timing workload differs'
            assert fixture['bytes'] == size and fixture['retained_groups'] == groups
            samples = [x for x in records if x.get('phase') == phase]
            assert len(samples) == count and [x['sample'] for x in samples] == list(range(count))
            assert all(x['iterations'] == batch and x['checksum'] == size for x in samples)
            assert all(math.isfinite(x['wall_ms']) and x['wall_ms'] > 0 for x in samples)
            row[label + '_median_ms'] = statistics.median(x['wall_ms'] for x in samples)
            row[label + '_cpu_median_ms'] = statistics.median(x['cpu_ms'] for x in samples)
            row[label + '_process_peak_rss_kib'] = max(x['process_peak_rss_kib'] for x in samples)
            row[label + '_process_seconds'] = time.monotonic() - started
            row[label + '_raw_sha256'] = digest(raw)
        result['pairs'].append(row)
        result['fixture'] = fixture
        path.write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(row), flush=True)
    result['summary'] = summarize(result['pairs'], args.seed)
    result['status'] = 'MEASURED_NOT_A_GUI_OR_FULL_APP_PASS'
    path.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result['summary']), flush=True)


if __name__ == '__main__':
    main()
