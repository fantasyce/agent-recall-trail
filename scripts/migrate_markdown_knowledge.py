#!/usr/bin/env python3
"""Migrate a reviewed Markdown tree into immutable ART Knowledge Editions."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import sqlite3
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from typing import Any


SCHEMA = "art.knowledge.migration.receipt.v1"


def run_json(arguments: list[str]) -> dict[str, Any]:
    completed = subprocess.run(arguments, check=False, capture_output=True, text=True)
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or "ART command failed")
    return json.loads(completed.stdout)


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_digest(items: list[dict[str, Any]]) -> str:
    digest = hashlib.sha256()
    for item in sorted(items, key=lambda value: value["source_path"]):
        digest.update(item["source_path"].encode("utf-8"))
        digest.update(b"\0")
        digest.update(item["source_sha256"].encode("ascii"))
        digest.update(b"\n")
    return digest.hexdigest()


def write_receipt(path: pathlib.Path, receipt: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    encoded = (json.dumps(receipt, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(descriptor, 0o600)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def materialized_edition(home: pathlib.Path, proposal_id: str) -> dict[str, str]:
    database = home / "data" / "art" / "knowledge-vault" / "art-control.sqlite3"
    with sqlite3.connect(database) as connection:
        row = connection.execute(
            """
            SELECT e.edition_id, e.knowledge_key, e.markdown_path, e.manifest_path
            FROM publish_intents p
            JOIN edition_projections e ON e.edition_id = p.edition_id
            WHERE p.proposal_id = ? AND p.state = 'committed'
            ORDER BY e.published_at DESC LIMIT 1
            """,
            (proposal_id,),
        ).fetchone()
    if row is None:
        raise RuntimeError(f"materialized proposal has no committed Edition: {proposal_id}")
    return {
        "edition_id": row[0],
        "knowledge_key": row[1],
        "markdown_path": row[2],
        "manifest_path": row[3],
    }


def stable_key(relative_path: str) -> str:
    return "import." + hashlib.sha256(relative_path.encode("utf-8")).hexdigest()[:32]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--art", required=True, type=pathlib.Path)
    parser.add_argument("--home", required=True, type=pathlib.Path)
    parser.add_argument("--source", required=True, type=pathlib.Path)
    parser.add_argument("--agent", required=True)
    parser.add_argument("--receipt", required=True, type=pathlib.Path)
    parser.add_argument("--confirm-reviewed", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.confirm_reviewed:
        raise RuntimeError("migration requires --confirm-reviewed from the local operator")
    art = str(args.art.resolve(strict=True))
    home = args.home.resolve(strict=True)
    source = args.source.resolve(strict=True)
    receipt_path = args.receipt.resolve()

    scan = run_json([art, "--home", str(home), "import", "markdown", "--source", str(source), "--dry-run"])
    proposals = scan["knowledge_import"]["proposals"]
    blocked = [item["source_path"] for item in proposals if not item["eligible"]]
    if blocked:
        raise RuntimeError(f"blocked Markdown findings must be resolved: {blocked}")

    inventory = []
    for proposal in sorted(proposals, key=lambda item: item["source_path"]):
        relative = pathlib.PurePosixPath(proposal["source_path"]).as_posix()
        inventory.append(
            {
                "source_path": relative,
                "source_sha256": proposal["content_sha256"],
                "title": proposal.get("title") or pathlib.PurePosixPath(relative).stem,
                "warnings": proposal.get("warnings", []),
                "knowledge_key": stable_key(relative),
            }
        )

    if receipt_path.exists():
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        if receipt.get("schema") != SCHEMA:
            raise RuntimeError("existing migration receipt has an unsupported schema")
        if receipt.get("source_tree_sha256") != tree_digest(inventory):
            raise RuntimeError("source inventory differs from the existing migration receipt")
    else:
        receipt = {
            "schema": SCHEMA,
            "created_at": datetime.now(timezone.utc).isoformat(),
            "source_root": str(source),
            "source_file_count": len(inventory),
            "source_tree_sha256": tree_digest(inventory),
            "agent": args.agent,
            "status": "in_progress",
            "items": [],
        }
        write_receipt(receipt_path, receipt)

    completed = {item["source_path"]: item for item in receipt["items"]}
    for item in inventory:
        previous = completed.get(item["source_path"])
        if previous and previous.get("status") == "published":
            continue
        markdown = source / pathlib.Path(item["source_path"])
        if sha256_file(markdown) != item["source_sha256"]:
            raise RuntimeError(f"source changed after scan: {item['source_path']}")
        idempotency_key = "markdown-import-" + hashlib.sha256(
            f"{item['source_path']}\0{item['source_sha256']}".encode("utf-8")
        ).hexdigest()
        proposal = run_json(
            [
                art,
                "--home",
                str(home),
                "knowledge",
                "proposal",
                "compose-file",
                "--agent",
                args.agent,
                "--knowledge-key",
                item["knowledge_key"],
                "--title",
                item["title"],
                "--applicability",
                "Reviewed local Agent knowledge; apply only within the document's stated scope.",
                "--markdown-file",
                str(markdown),
                "--source-id",
                item["source_path"],
                "--source-sha256",
                item["source_sha256"],
                "--idempotency-key",
                idempotency_key,
            ]
        )
        if proposal["status"] == "submitted":
            run_json(
                [
                    art,
                    "--home",
                    str(home),
                    "knowledge",
                    "review",
                    "approve",
                    proposal["id"],
                    "--revision",
                    str(proposal["revision"]),
                    "--reason",
                    "Reviewed for local KnowledgeBase migration; source digest and content accepted.",
                ]
            )
            proposal["status"] = "approved"
        if proposal["status"] == "approved":
            edition = run_json(
                [
                    art,
                    "--home",
                    str(home),
                    "knowledge",
                    "publish",
                    proposal["id"],
                    "--revision",
                    str(proposal["revision"]),
                    "--confirm",
                ]
            )
        elif proposal["status"] == "materialized":
            edition = materialized_edition(home, proposal["id"])
        else:
            raise RuntimeError(f"proposal cannot be migrated from state {proposal['status']}")
        record = {
            **item,
            "proposal_id": proposal["id"],
            "proposal_revision": proposal["revision"],
            "edition_id": edition["edition_id"],
            "markdown_path": edition["markdown_path"],
            "manifest_path": edition["manifest_path"],
            "status": "published",
        }
        receipt["items"] = [
            existing for existing in receipt["items"] if existing["source_path"] != item["source_path"]
        ] + [record]
        receipt["items"].sort(key=lambda value: value["source_path"])
        write_receipt(receipt_path, receipt)

    receipt["status"] = "complete"
    write_receipt(receipt_path, receipt)
    print(json.dumps(receipt, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (KeyError, OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"migration failed: {error}", file=sys.stderr)
        raise SystemExit(1)
