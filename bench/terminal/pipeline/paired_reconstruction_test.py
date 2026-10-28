"""Statistics helper tests only; these are not performance measurements."""
import unittest
from paired_reconstruction import summarize


class PairedStatisticsTests(unittest.TestCase):
    def pairs(self, ratios):
        return [{'baseline_median_ms': 2.0, 'head_median_ms': 2.0 * value} for value in ratios]

    def test_identity_control(self):
        result = summarize(self.pairs([1.0] * 4), 7)
        self.assertEqual(result['head_over_baseline_geomean_ratio'], 1.0)
        self.assertEqual(result['paired_bootstrap_95pct_ratio'], [1.0, 1.0])

    def test_direction_and_units(self):
        result = summarize(self.pairs([1.06] * 4), 7)
        self.assertAlmostEqual(result['head_over_baseline_geomean_ratio'], 1.06)
        self.assertEqual(result['baseline_process_median_range_ms'], [2.0, 2.0])
        self.assertEqual(result['head_process_median_range_ms'], [2.12, 2.12])

    def test_pair_level_variation_and_determinism(self):
        pairs = self.pairs([0.90, 0.98, 1.02, 1.10])
        result = summarize(pairs, 7)
        self.assertEqual(result, summarize(pairs, 7))
        low, high = result['paired_bootstrap_95pct_ratio']
        self.assertLess(low, 1.0)
        self.assertGreater(high, 1.0)
        self.assertEqual(result['process_pairs'], 4)


if __name__ == '__main__':
    unittest.main()
