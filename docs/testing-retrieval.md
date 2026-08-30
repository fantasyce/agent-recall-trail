# Retrieval quality testing

ART uses BEIR SciFact and NFCorpus as public, repeatable lexical retrieval
quality gates. SciFact emphasizes precise retrieval with few relevant documents;
NFCorpus emphasizes recall when one query has many relevant documents.

Download into a new task-owned directory:

```bash
bash scripts/download_beir_retrieval_fixtures.sh /private/tmp/art-beir-fixtures
```

The script downloads only the official BEIR archives and verifies these
published MD5 values before extraction:

- SciFact: `5f7d1de60b170fc8027bb7898e2efca1`
- NFCorpus: `a89dba18a62ef92f7d323ec890a0d38d`

Dataset owners retain their own licenses. BEIR's distribution does not replace
the operator's responsibility to review and follow those licenses.

Run the compiled ART product path and write a new aggregate result:

```bash
bash scripts/run_beir_retrieval_benchmark.sh \
  /private/tmp/art-beir-fixtures \
  /private/tmp/art-beir-results.json
```

The harness creates disposable ART homes, projects public documents into a
benchmark-only Knowledge Vault, executes one `art recall` process per query with
`--budget-tokens 6000 --max-knowledge-results 10`, computes macro Recall,
Precision, MRR, nDCG, and Accuracy at 1/3/10, and removes the generated Vaults.
It never reads or writes the formal ART home.

Release gates:

| Dataset | Recall@10 | nDCG@10 | nDCG@3 | Product p95 |
|---|---:|---:|---:|---:|
| SciFact | >= 0.76 | >= 0.64 | >= 0.4375 | < 350 ms |
| NFCorpus | >= 0.14 | >= 0.29 | >= 0.2226 | < 350 ms |

The checked-in repository contains only the harness, checksums, metric
definitions, and aggregate acceptance evidence. Do not add the archives,
corpora, queries, qrels, generated Vaults, or raw run files to Git.
