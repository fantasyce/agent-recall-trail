#!/usr/bin/env python3
"""Task-owned stdio lifecycle and pressure acceptance for ART."""

from __future__ import annotations

import concurrent.futures
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def request(identifier: int, method: str, params: dict) -> str:
    return json.dumps(
        {"jsonrpc": "2.0", "id": identifier, "method": method, "params": params},
        separators=(",", ":"),
    )


def notification(method: str, params: dict) -> str:
    return json.dumps(
        {"jsonrpc": "2.0", "method": method, "params": params},
        separators=(",", ":"),
    )


def initialize(identifier: int = 1) -> str:
    return request(
        identifier,
        "initialize",
        {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "art-stress", "version": "1"},
        },
    )


def serve_command(binary: Path, home: Path) -> list[str]:
    return [str(binary), "--home", str(home), "mcp", "serve", "--agent", "codex-primary"]


def one_session(binary: Path, home: Path, calls: int = 1) -> None:
    lines = [initialize(), notification("notifications/initialized", {})]
    lines.extend(
        request(
            index + 2,
            "tools/call",
            {
                "name": "art_recall",
                "arguments": {"query": "pressure query", "budget_tokens": 1800},
            },
        )
        for index in range(calls)
    )
    completed = subprocess.run(
        serve_command(binary, home),
        input="\n".join(lines) + "\n",
        text=True,
        capture_output=True,
        timeout=10,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr)
    responses = [json.loads(line) for line in completed.stdout.splitlines()]
    if not responses or any(value.get("jsonrpc") != "2.0" for value in responses):
        raise RuntimeError("stdout contained a non-JSON-RPC line")
    response_ids = {value.get("id") for value in responses}
    if 1 not in response_ids or calls + 1 not in response_ids:
        raise RuntimeError("missing pressure response")


def abnormal_disconnect(binary: Path, home: Path) -> None:
    process = subprocess.Popen(
        serve_command(binary, home),
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    assert process.stdin is not None
    process.stdin.write(initialize() + "\n")
    process.stdin.flush()
    process.stdin.write('{"jsonrpc":"2.0","id":')
    process.stdin.close()
    process.wait(timeout=3)
    if process.returncode != 0:
        stderr = process.stderr.read() if process.stderr else ""
        raise RuntimeError(f"abnormal disconnect did not close cleanly: {stderr}")


def idle_fd_count(binary: Path, home: Path) -> int | None:
    process = subprocess.Popen(
        serve_command(binary, home),
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    try:
        assert process.stdin is not None
        process.stdin.write(initialize() + "\n")
        process.stdin.flush()
        time.sleep(0.1)
        proc_fd = Path(f"/proc/{process.pid}/fd")
        if proc_fd.exists():
            return len(list(proc_fd.iterdir()))
        result = subprocess.run(
            ["lsof", "-a", "-p", str(process.pid), "-d", "0-999", "-Fn"],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode == 0:
            return sum(1 for line in result.stdout.splitlines() if line.startswith("f"))
        return None
    finally:
        if process.stdin:
            process.stdin.close()
        process.wait(timeout=3)


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: stress_gate.py /absolute/path/to/art")
    binary = Path(sys.argv[1]).resolve()
    if not binary.is_file():
        raise SystemExit("release binary is missing")
    with tempfile.TemporaryDirectory(prefix="art-stress-") as temporary:
        home = Path(temporary)
        subprocess.run(
            [str(binary), "--home", str(home), "init", "--confirm"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        subprocess.run(
            [
                str(binary),
                "--home",
                str(home),
                "agent",
                "create",
                "--id",
                "codex-primary",
                "--host",
                "codex",
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        started = time.monotonic()
        for _ in range(500):
            one_session(binary, home)
        graceful_seconds = time.monotonic() - started
        for _ in range(100):
            abnormal_disconnect(binary, home)
        started = time.monotonic()
        one_session(binary, home, calls=1000)
        query_seconds = time.monotonic() - started
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            futures = [executor.submit(one_session, binary, home, 20) for _ in range(8)]
            for future in futures:
                future.result(timeout=10)
        doctor = subprocess.run(
            [
                str(binary),
                "--home",
                str(home),
                "doctor",
                "--agent",
                "codex-primary",
                "--json",
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        status = json.loads(doctor.stdout)
        if status.get("status") != "ok":
            raise RuntimeError("doctor degraded after pressure")
        fd_count = idle_fd_count(binary, home)
        if fd_count is not None and fd_count > 16:
            raise RuntimeError(f"idle descriptor target exceeded: {fd_count}")
        print(
            json.dumps(
                {
                    "schema": "art.stress.v1",
                    "graceful_sessions": 500,
                    "graceful_seconds": round(graceful_seconds, 3),
                    "abnormal_disconnects": 100,
                    "queries_one_process": 1000,
                    "query_seconds": round(query_seconds, 3),
                    "concurrent_clients": 8,
                    "idle_fd_count": fd_count,
                    "doctor": "ok",
                },
                separators=(",", ":"),
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
