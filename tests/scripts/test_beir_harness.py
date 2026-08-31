#!/usr/bin/env python3
import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "benchmark_beir_retrieval.py"
SPEC = importlib.util.spec_from_file_location("benchmark_beir_retrieval", SCRIPT)
BENCHMARK = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BENCHMARK)


class BeirHarnessContracts(unittest.TestCase):
    def test_semantic_qualification_rejects_any_fallback_or_degraded_query(self):
        self.assertTrue(
            BENCHMARK.execution_is_qualified("semantic", {"semantic"}, {"ready"})
        )
        self.assertFalse(
            BENCHMARK.execution_is_qualified(
                "semantic", {"semantic", "lexical"}, {"ready"}
            )
        )
        self.assertFalse(
            BENCHMARK.execution_is_qualified(
                "hybrid", {"hybrid"}, {"ready", "degraded"}
            )
        )

    def test_reported_provider_fingerprint_must_match_the_loaded_endpoint(self):
        fingerprint = "a" * 64
        self.assertEqual(
            BENCHMARK.bind_provider_fingerprint(fingerprint, fingerprint), fingerprint
        )
        with self.assertRaises(ValueError):
            BENCHMARK.bind_provider_fingerprint(fingerprint, "b" * 64)

    def test_full_scan_is_reported_as_a_core_local_product_path(self):
        self.assertEqual(BENCHMARK.qualification_scope("lexical"), "art_default")
        self.assertEqual(BENCHMARK.qualification_scope("full_scan"), "art_full_scan")
        self.assertEqual(
            BENCHMARK.qualification_scope("semantic"), "provider_specific"
        )


if __name__ == "__main__":
    unittest.main()
