#!/usr/bin/env python3
"""Reconcile an ART Markdown migration receipt against sources and Editions."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sqlite3
import subprocess
import sys
from typing import Any


SCHEMA = "art.knowledge.migration.receipt.v1"


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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--art", required=True, type=pathlib.Path)
    parser.add_argument("--home", required=True, type=pathlib.Path)
    parser.add_argument("--source", type=pathlib.Path)
    parser.add_argument("--receipt", required=True, type=pathlib.Path)
    parser.add_argument("--allow-source-absent", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    art = str(args.art.resolve(strict=True))
    home = args.home.resolve(strict=True)
    receipt = json.loads(args.receipt.resolve(strict=True).read_text(encoding="utf-8"))
    if receipt.get("schema") != SCHEMA or receipt.get("status") != "complete":
        raise RuntimeError("migration receipt is not complete or has an unsupported schema")
    items = receipt.get("items", [])
    if len(items) != receipt.get("source_file_count"):
        raise RuntimeError("receipt count does not match its source inventory")
    for key in ("source_path", "knowledge_key", "proposal_id", "edition_id"):
        values = [item[key] for item in items]
        if len(values) != len(set(values)):
            raise RuntimeError(f"duplicate {key} in migration receipt")
    if tree_digest(items) != receipt.get("source_tree_sha256"):
        raise RuntimeError("receipt source tree digest is inconsistent")

    if args.source is not None:
        source = args.source.resolve()
        if not source.exists():
            if not args.allow_source_absent:
                raise RuntimeError("source root is absent")
        else:
            markdown = sorted(path.relative_to(source).as_posix() for path in source.rglob("*.md"))
            if markdown != sorted(item["source_path"] for item in items):
                raise RuntimeError("source Markdown inventory does not match the receipt")
            for item in items:
                candidate = (source / item["source_path"]).resolve(strict=True)
                candidate.relative_to(source)
                if sha256_file(candidate) != item["source_sha256"]:
                    raise RuntimeError(f"source digest mismatch: {item['source_path']}")

    knowledge_root = (home / "data" / "art" / "knowledge-vault").resolve(strict=True)
    database = knowledge_root / "art-control.sqlite3"
    with sqlite3.connect(database) as connection:
        current = connection.execute(
            "SELECT edition_id FROM edition_projections WHERE current=1 AND revoked=0"
        ).fetchall()
    current_ids = {row[0] for row in current}
    expected_ids = {item["edition_id"] for item in items}
    if current_ids != expected_ids:
        raise RuntimeError("current Knowledge Edition set does not match the migration receipt")

    for item in items:
        if item.get("status") != "published":
            raise RuntimeError(f"item is not published: {item['source_path']}")
        manifest_path = pathlib.Path(item["manifest_path"]).resolve(strict=True)
        markdown_path = pathlib.Path(item["markdown_path"]).resolve(strict=True)
        manifest_path.relative_to(knowledge_root)
        markdown_path.relative_to(knowledge_root)
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if manifest["edition_id"] != item["edition_id"]:
            raise RuntimeError(f"manifest Edition mismatch: {item['source_path']}")
        if manifest["knowledge_key"] != item["knowledge_key"]:
            raise RuntimeError(f"manifest key mismatch: {item['source_path']}")
        if manifest["markdown_body_sha256"] != item["source_sha256"]:
            raise RuntimeError(f"Edition body mismatch: {item['source_path']}")
        verified = subprocess.run(
            [
                art,
                "--home",
                str(home),
                "knowledge",
                "verify",
                "--edition",
                item["edition_id"],
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        if verified.returncode != 0:
            raise RuntimeError(verified.stderr.strip() or f"Edition verify failed: {item['edition_id']}")

    print(
        json.dumps(
            {
                "schema": "art.knowledge.migration.verification.v1",
                "status": "ok",
                "source_file_count": len(items),
                "edition_count": len(current_ids),
                "source_available": bool(args.source and args.source.exists()),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (KeyError, OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"verification failed: {error}", file=sys.stderr)
        raise SystemExit(1)
