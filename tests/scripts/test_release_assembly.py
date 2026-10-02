#!/usr/bin/env python3
"""Aggregation must consume, never rebuild, the verified native archives."""
import hashlib
import importlib.util
import pathlib
import subprocess
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = REPO / "scripts/build_release_assets.py"
SPEC = importlib.util.spec_from_file_location("release_assets", SCRIPT)
ASSETS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ASSETS)


class AssemblyContract(unittest.TestCase):
    def test_existing_native_bytes_are_preserved_and_missing_input_fails(self):
        with tempfile.TemporaryDirectory(prefix="art-assembly-") as tmp:
            dist = pathlib.Path(tmp)
            binary = dist / "fixture-binary"
            binary.write_bytes(b"synthetic release assembly fixture")
            commit = "a" * 40
            args = ["python3", str(SCRIPT), "--repo", str(REPO), "--dist", str(dist),
                    "--version", "0.3.7", "--commit", commit, "--assemble-only"]
            ASSETS.tar_asset(REPO, dist, "0.3.7", commit, "darwin_arm64", binary)
            failed = subprocess.run(args, capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn("missing native archive", failed.stderr)
            ASSETS.tar_asset(REPO, dist, "0.3.7", commit, "linux_amd64", binary)
            binary.unlink()
            before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                      for p in dist.glob("*.tar.gz")}
            subprocess.run(args, check=True, capture_output=True)
            self.assertEqual(before, {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                                     for p in dist.glob("*.tar.gz")})
            self.assertTrue((dist / "agent-recall-trail_0.3.7.mcpb").is_file())
            verify = ["bash", str(REPO / "scripts/verify_release_assets.sh"),
                      str(dist), "0.3.7", commit]
            subprocess.run(verify, check=True, capture_output=True)
            binary.write_bytes(b"fixture " + b"/" + b"Users/" + b"synthetic/private-path")
            ASSETS.tar_asset(REPO, dist, "0.3.7", commit, "linux_amd64", binary)
            binary.unlink()
            subprocess.run(args, check=True, capture_output=True)
            self.assertNotEqual(subprocess.run(verify, capture_output=True).returncode, 0)
            binary.write_bytes(b"fixture " + b"gh" + b"p_" + b"synthetic" * 5)
            ASSETS.tar_asset(REPO, dist, "0.3.7", commit, "linux_amd64", binary)
            binary.unlink()
            subprocess.run(args, check=True, capture_output=True)
            self.assertNotEqual(subprocess.run(verify, capture_output=True).returncode, 0)


if __name__ == "__main__":
    unittest.main()
