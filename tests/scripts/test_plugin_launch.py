"""Exercise the packaged MCP launch boundary without a login shell."""

import json
import os
from pathlib import Path
import re
import select
import subprocess
import tempfile
import time
import unittest


PLUGIN = Path(os.environ.get(
    "ART_PLUGIN_ROOT", Path(__file__).resolve().parents[2] / "plugin/agent-recall-trail"
)).resolve()
BINARY = Path(os.environ.get("ART_BIN", "target/release/art")).resolve()
CONFIG = json.loads((PLUGIN / ".mcp.json").read_text())["mcpServers"]["agent-recall-trail"]
COMMAND = [CONFIG["command"], *CONFIG["args"]]
CWD = PLUGIN / CONFIG.get("cwd", ".")


class PluginLaunchTests(unittest.TestCase):
    def test_default_install_is_discovered_with_a_minimal_host_path(self):
        self.check_launch(default_install=True)

    def test_custom_path_install_remains_supported(self):
        self.check_launch(default_install=False)

    def test_path_installation_takes_precedence_and_preserves_arguments(self):
        with tempfile.TemporaryDirectory(prefix="art-plugin-precedence-") as temporary:
            home = Path(temporary)
            local = home / ".local/bin"
            custom = home / "custom bin"
            local.mkdir(parents=True)
            custom.mkdir()
            (local / "art").symlink_to(BINARY)
            executable = custom / "art"
            executable.write_text('#!/bin/sh\nprintf \'%s\\n\' "$@"\nexit 23\n')
            executable.chmod(0o700)
            arguments = ["value with spaces", "literal*argument"]
            result = subprocess.run(
                [*COMMAND, *arguments], cwd=CWD,
                env={"HOME": str(home), "PATH": str(custom)},
                input="", text=True, capture_output=True, timeout=10,
            )
            self.assertEqual(result.returncode, 23)
            self.assertEqual(result.stdout.splitlines(), [*CONFIG["args"][1:], *arguments])
            self.assertEqual(result.stderr, "")

    def test_missing_executable_fails_without_stdout(self):
        self.check_unavailable("missing")

    def test_nonexecutable_install_fails_without_stdout(self):
        self.check_unavailable("nonexecutable")

    def test_broken_install_link_fails_without_stdout(self):
        self.check_unavailable("broken")

    def test_missing_home_fails_without_stdout(self):
        self.check_unavailable("no-home")

    def check_unavailable(self, state):
        with tempfile.TemporaryDirectory(prefix="art-plugin-missing-") as temporary:
            home = Path(temporary)
            empty_path = home / "empty-path"
            empty_path.mkdir()
            local = home / ".local/bin/art"
            local.parent.mkdir(parents=True)
            if state == "nonexecutable":
                local.write_text("not executable")
                local.chmod(0o600)
            elif state == "broken":
                local.symlink_to(home / "absent")
            environment = {"PATH": str(empty_path)}
            if state != "no-home":
                environment["HOME"] = str(home)
            result = subprocess.run(COMMAND, cwd=CWD, env=environment,
                                    input="", text=True, capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 127)
            self.assertEqual(result.stdout, "")
            self.assertIn("ART executable unavailable", result.stderr)
            self.assertFalse((home / ".across").exists())

    def test_missing_agent_is_not_created_implicitly(self):
        with tempfile.TemporaryDirectory(prefix="art-plugin-agent-") as temporary:
            home = Path(temporary)
            local = home / ".local/bin/art"
            local.parent.mkdir(parents=True)
            local.symlink_to(BINARY)
            art_home = home / ".across"
            environment = {"HOME": str(home), "PATH": "/usr/bin:/bin"}
            subprocess.run([str(BINARY), "--home", str(art_home), "init", "--confirm"],
                           env=environment, check=True, capture_output=True, timeout=10)
            result = subprocess.run([*COMMAND, "--home", str(art_home)], cwd=CWD,
                                    env=environment, input="", text=True,
                                    capture_output=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")
            self.assertFalse((art_home / "config/art/agents/codex-primary.json").exists())
            self.assertFalse((art_home / "data/art/agents/codex-primary/art.sqlite3").exists())

    def check_launch(self, default_install):
        self.assertTrue(BINARY.is_file(), "Set ART_BIN to a built ART executable")
        with tempfile.TemporaryDirectory(prefix="art-plugin-launch-") as temporary:
            home = Path(temporary) / "user home"
            art_home = home / ".across"
            bin_dir = home / (".local/bin" if default_install else "custom bin")
            bin_dir.mkdir(parents=True)
            (bin_dir / "art").symlink_to(BINARY)
            environment = {"HOME": str(home), "PATH": "/usr/bin:/bin"}
            if not default_install:
                environment["PATH"] = str(bin_dir) + ":" + environment["PATH"]
            for arguments in [
                ["init", "--confirm"],
                ["agent", "create", "--id", "codex-primary", "--host", "codex"],
            ]:
                subprocess.run(
                    [str(BINARY), "--home", str(art_home), *arguments],
                    env=environment, check=True, capture_output=True, timeout=10,
                )
            # A new child must reconnect after the preceding child exits on EOF.
            for _ in range(2):
                self.check_session(art_home, environment)

    def check_session(self, art_home, environment):
        try:
            process = subprocess.Popen(
                [*COMMAND, "--home", str(art_home)],
                cwd=CWD, env=environment,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=subprocess.PIPE, text=True,
            )
        except OSError as error:
            self.fail(f"Plugin cannot start in a minimal host environment: {error}")
        try:
            self.request(process, 1, "initialize", {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "art-launch-contract", "version": "1"},
            })
            process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
            process.stdin.flush()
            tools = self.request(process, 2, "tools/list", {})["tools"]
            names = {tool["name"] for tool in tools}
            self.assertEqual(names, {
                "art_feedback", "art_governance_ui_open", "art_health",
                "art_knowledge_governance", "art_knowledge_propose",
                "art_memory_capture", "art_read", "art_recall",
            })
            # A skill naming an unavailable operation is a broken consumer contract.
            references = set(re.findall(r"`(art_[a-z_]+)`", (PLUGIN / "skills/agent-recall-trail/SKILL.md").read_text()))
            self.assertFalse(references - names, f"Skill references unavailable tools: {references - names}")
            health = self.request(process, 3, "tools/call", {"name": "art_health", "arguments": {}})
            self.assertFalse(health.get("isError", False))
            self.assertEqual(health["structuredContent"]["bound_agent_id"], "codex-primary")
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            process.stdout.close()
            stderr = process.stderr.read()
            process.stderr.close()
        self.assertEqual(process.returncode, 0, stderr)
        self.assertEqual(stderr, "")

    def request(self, process, identifier, method, params):
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params}) + "\n")
        process.stdin.flush()
        deadline = time.monotonic() + 10
        line = bytearray()
        while not line.endswith(b"\n"):
            remaining = deadline - time.monotonic()
            self.assertGreater(remaining, 0, "MCP response timed out")
            self.assertTrue(select.select([process.stdout], [], [], remaining)[0], "MCP response timed out")
            byte = os.read(process.stdout.fileno(), 1)
            self.assertTrue(byte, "MCP exited before responding")
            line.extend(byte)
        response = json.loads(line)
        self.assertEqual(response.get("jsonrpc"), "2.0")
        self.assertEqual(response.get("id"), identifier)
        self.assertNotIn("error", response)
        return response["result"]


if __name__ == "__main__":
    unittest.main()
