#!/usr/bin/env python3
"""Evaluate the compiled ART recall path on public BEIR retrieval datasets."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import re
import shutil
import sqlite3
import statistics
import subprocess
import tempfile
import time
import unicodedata
from collections import defaultdict
from pathlib import Path


GATES = {
    "scifact": {"recall@10": 0.76, "ndcg@10": 0.64, "ndcg@3": 0.4375},
    "nfcorpus": {"recall@10": 0.14, "ndcg@10": 0.29, "ndcg@3": 0.2226},
}


def run(command: list[str]) -> str:
    completed = subprocess.run(command, check=True, text=True, capture_output=True)
    return completed.stdout


def load_jsonl(path: Path) -> list[dict]:
    with path.open(encoding="utf-8") as handle:
        return [json.loads(line) for line in handle if line.strip()]


def load_qrels(path: Path) -> dict[str, dict[str, int]]:
    qrels: dict[str, dict[str, int]] = defaultdict(dict)
    with path.open(encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            qrels[row["query-id"]][row["corpus-id"]] = int(row["score"])
    return dict(qrels)


def search_document(markdown: str) -> str:
    normalized = unicodedata.normalize("NFKC", markdown).lower()
    cjk = [
        value
        for value in normalized
        if 0x3400 <= ord(value) <= 0x4DBF
        or 0x4E00 <= ord(value) <= 0x9FFF
        or 0xF900 <= ord(value) <= 0xFAFF
    ]
    return f"{normalized}\n{' '.join(a + b for a, b in zip(cjk, cjk[1:]))}"


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[round((len(ordered) - 1) * fraction)]


def execution_is_qualified(
    requested_mode: str, effective_modes: set[str], vector_statuses: set[str]
) -> bool:
    if effective_modes != {requested_mode}:
        return False
    return requested_mode not in {"semantic", "hybrid"} or vector_statuses == {"ready"}


def bind_provider_fingerprint(expected: str, actual: str) -> str:
    valid = lambda value: len(value) == 64 and all(
        character in "0123456789abcdef" for character in value
    )
    if not valid(expected) or not valid(actual) or expected != actual:
        raise ValueError(
            "provider fingerprint does not match the endpoint loaded by ART"
        )
    return actual


def metric_at_k(ranking: list[str], relevant: dict[str, int], k: int) -> dict[str, float]:
    ranking = ranking[:k]
    positive = {document: score for document, score in relevant.items() if score > 0}
    hits = [document for document in ranking if document in positive]
    first = next(
        (index + 1 for index, document in enumerate(ranking) if document in positive),
        None,
    )
    dcg = sum(
        (2 ** positive.get(document, 0) - 1) / math.log2(index + 2)
        for index, document in enumerate(ranking)
    )
    ideal = sorted(positive.values(), reverse=True)[:k]
    idcg = sum((2**score - 1) / math.log2(index + 2) for index, score in enumerate(ideal))
    return {
        "recall": len(hits) / max(1, len(positive)),
        "precision": len(hits) / k,
        "mrr": 0.0 if first is None else 1.0 / first,
        "ndcg": 0.0 if idcg == 0 else dcg / idcg,
        "accuracy": float(bool(hits)),
    }


def aggregate(rankings: dict[str, list[str]], qrels: dict[str, dict[str, int]]) -> dict:
    result = {}
    for k in (1, 3, 10):
        rows = [
            metric_at_k(rankings.get(query_id, []), relevant, k)
            for query_id, relevant in qrels.items()
        ]
        result[f"@{k}"] = {
            name: round(statistics.fmean(row[name] for row in rows), 6)
            for name in ("recall", "precision", "mrr", "ndcg", "accuracy")
        }
    return result


def create_fixture(
    art: Path,
    dataset: str,
    source: Path,
    work: Path,
    mode: str,
    embedding_config: Path | None,
) -> tuple[Path, dict[str, str], str | None]:
    home = work / f"home-{dataset}"
    run([str(art), "init", "--confirm", "--home", str(home)])
    run(
        [
            str(art),
            "agent",
            "--home",
            str(home),
            "create",
            "--id",
            "benchmark-agent",
            "--host",
            "codex",
        ]
    )
    knowledge = home / "data" / "art" / "knowledge-vault"
    connection = sqlite3.connect(knowledge / "art-control.sqlite3")
    edition_to_document = {}
    try:
        connection.execute("BEGIN")
        for index, document in enumerate(load_jsonl(source / "corpus.jsonl")):
            edition_id = f"arke_beir_{dataset}_{index:06d}"
            document_id = str(document["_id"])
            edition_to_document[edition_id] = document_id
            key = f"beir.{dataset}.doc-{index:06d}"
            directory = knowledge / "editions" / key
            directory.mkdir(parents=True)
            title = document.get("title") or document_id
            body = f"# {title}\n\n{document.get('text', '')}\n"
            body_hash = hashlib.sha256(body.encode()).hexdigest()
            manifest = {
                "schema": "art.knowledge.edition.v1",
                "edition_id": edition_id,
                "knowledge_key": key,
                "edition_number": 1,
                "title": title,
                "markdown_body_sha256": body_hash,
                "source_set_hash": "a" * 64,
                "source_commitments": ["b" * 64],
                "review_receipt_hash": "c" * 64,
                "published_at": "2026-08-30T00:00:00Z",
                "generator": "art-beir-release-gate",
            }
            manifest_bytes = json.dumps(manifest, ensure_ascii=False, indent=2).encode()
            manifest_hash = hashlib.sha256(manifest_bytes).hexdigest()
            stem = f"1-{edition_id}"
            manifest_path = directory / f"{stem}.json"
            markdown_path = directory / f"{stem}.md"
            markdown = f"---\nmanifest_sha256: {manifest_hash}\n---\n\n{body}"
            manifest_path.write_bytes(manifest_bytes)
            markdown_path.write_text(markdown, encoding="utf-8")
            connection.execute(
                "INSERT INTO edition_projections(edition_id,knowledge_key,edition_number,title,markdown_path,manifest_path,markdown_sha256,manifest_sha256,published_at,revoked,current) VALUES (?,?,1,?,?,?,?,?,'2026-08-30T00:00:00Z',0,1)",
                (
                    edition_id,
                    key,
                    title,
                    str(markdown_path),
                    str(manifest_path),
                    hashlib.sha256(markdown.encode()).hexdigest(),
                    manifest_hash,
                ),
            )
            connection.execute(
                "INSERT INTO knowledge_fts(edition_id,search_text) VALUES (?,?)",
                (edition_id, search_document(markdown)),
            )
        connection.commit()
    finally:
        connection.close()
    if mode in {"semantic", "hybrid"}:
        if embedding_config is None:
            raise SystemExit("semantic and hybrid evaluation require --embedding-config")
        target = home / "config" / "art" / "embedding" / "default.json"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(embedding_config, target)
        target.chmod(0o600)
        reindex = json.loads(
            run(
            [
                str(art),
                "--home",
                str(home),
                "reindex",
                "--agent",
                "benchmark-agent",
                "--knowledge",
                "--vectors",
            ]
            )
        )
        provider_fingerprint = reindex.get("provider_fingerprint")
        if not isinstance(provider_fingerprint, str):
            raise ValueError("ART vector reindex did not report its provider fingerprint")
    else:
        provider_fingerprint = None
    return home, edition_to_document, provider_fingerprint


def evaluate_dataset(
    art: Path,
    dataset: str,
    source: Path,
    work: Path,
    mode: str,
    embedding_config: Path | None,
    provider_fingerprint: str | None,
) -> dict:
    qrels = load_qrels(source / "qrels" / "test.tsv")
    all_queries = {
        str(row["_id"]): row["text"] for row in load_jsonl(source / "queries.jsonl")
    }
    home, edition_to_document, actual_provider_fingerprint = create_fixture(
        art, dataset, source, work, mode, embedding_config
    )
    bound_provider_fingerprint = (
        bind_provider_fingerprint(
            provider_fingerprint or "", actual_provider_fingerprint or ""
        )
        if mode in {"semantic", "hybrid"}
        else None
    )
    rankings = {}
    latencies = []
    vector_statuses = set()
    effective_modes = set()
    for query_id in qrels:
        started = time.perf_counter()
        payload = json.loads(
            run(
                [
                    str(art),
                    "recall",
                    "--home",
                    str(home),
                    "--agent",
                    "benchmark-agent",
                    "--mode",
                    mode,
                    "--json",
                    "--budget-tokens",
                    "6000",
                    "--max-knowledge-results",
                    "10",
                    all_queries[query_id],
                ]
            )
        )
        latencies.append((time.perf_counter() - started) * 1000)
        vector_statuses.add(payload["vector_status"])
        effective_modes.add(payload["effective_mode"])
        rankings[query_id] = [
            edition_to_document[item["subject_ref"].removeprefix("knowledge:")]
            for item in payload["knowledge_editions"]
        ]
    metrics = aggregate(rankings, qrels)
    gates = GATES[dataset]
    execution_qualified = execution_is_qualified(
        mode, effective_modes, vector_statuses
    )
    passed = (
        metrics["@10"]["recall"] >= gates["recall@10"]
        and metrics["@10"]["ndcg"] >= gates["ndcg@10"]
        and metrics["@3"]["ndcg"] >= gates["ndcg@3"]
        and percentile(latencies, 0.95) < 350.0
        and execution_qualified
    )
    return {
        "dataset": dataset,
        "mode": mode,
        "provider_fingerprint": bound_provider_fingerprint,
        "corpus_documents": len(edition_to_document),
        "test_queries": len(qrels),
        "relevance_judgments": sum(len(values) for values in qrels.values()),
        "metrics": metrics,
        "latency_including_cli_startup": {
            "p50_ms": round(percentile(latencies, 0.50), 3),
            "p95_ms": round(percentile(latencies, 0.95), 3),
            "p99_ms": round(percentile(latencies, 0.99), 3),
        },
        "empty_result_queries": sum(not ranking for ranking in rankings.values()),
        "effective_mode": sorted(effective_modes),
        "vector_status": sorted(vector_statuses),
        "execution_qualified": execution_qualified,
        "passed": passed,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--art", required=True, type=Path)
    parser.add_argument("--datasets", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--mode",
        choices=("lexical", "full_scan", "semantic", "hybrid"),
        default="lexical",
    )
    parser.add_argument("--embedding-config", type=Path)
    parser.add_argument("--provider-fingerprint")
    args = parser.parse_args()
    if not args.art.is_file() or not args.datasets.is_dir() or args.output.exists():
        raise SystemExit("ART binary and dataset root must exist; output must be new")
    if args.mode in {"semantic", "hybrid"}:
        if args.embedding_config is None or not args.embedding_config.is_file():
            raise SystemExit("semantic and hybrid evaluation require --embedding-config")
        fingerprint = args.provider_fingerprint or ""
        if len(fingerprint) != 64 or any(value not in "0123456789abcdef" for value in fingerprint):
            raise SystemExit("semantic and hybrid evaluation require a lowercase SHA-256 --provider-fingerprint")
    elif args.embedding_config is not None or args.provider_fingerprint is not None:
        raise SystemExit("embedding qualification arguments require semantic or hybrid mode")
    for dataset in GATES:
        for relative in ("corpus.jsonl", "queries.jsonl", "qrels/test.tsv"):
            if not (args.datasets / dataset / relative).is_file():
                raise SystemExit(f"missing dataset file: {dataset}/{relative}")
    started = time.time()
    with tempfile.TemporaryDirectory(prefix="art-beir-release-gate-") as directory:
        results = [
            evaluate_dataset(
                args.art,
                dataset,
                args.datasets / dataset,
                Path(directory),
                args.mode,
                args.embedding_config,
                args.provider_fingerprint,
            )
            for dataset in GATES
        ]
    payload = {
        "schema": "art.beir.retrieval-gate.v1",
        "art_version": run([str(args.art), "--version"]).strip(),
        "mode": args.mode,
        "qualification_scope": (
            "art_default" if args.mode == "lexical" else "provider_specific"
        ),
        "provider_fingerprint": args.provider_fingerprint,
        "datasets": results,
        "elapsed_seconds": round(time.time() - started, 3),
        "passed": all(result["passed"] for result in results),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    if not payload["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
