# Unified memory value corpus

`unified-memory-value-v1.json` contains 64 distinct scenarios, half English and
half Chinese, for the shared private-memory value standard. Each of eight
classes has eight cases: verified conclusions, uncertain evidence, ordinary
negative content, malformed submissions, duplicates, conflicts, sensitive
content, and automatic revisions.

The `guidance` field is a human-authored expectation for a consuming Agent in
both ordinary proactive work and a Stop Hook continuation. `submit_for_review`
requires the Agent to preserve the stated uncertainty; `submit_revision`
requires an exact target revision. `skip` means no tool call. These labels are
not outputs from a classifier and are not evidence that an Agent followed the
instructions. Real-host evaluation must record the actual tool calls (or lack
of calls), receipts, and the exact installed guidance and runtime separately.

The `policy` field describes a deliberately submitted fixture's store outcome.
The Rust `bilingual_corpus_has_identical_policy_outcomes_for_proactive_and_hook_intake`
test runs every case through each real intake origin in a fresh temporary Vault.
It constructs hash-verifiable local evidence for verified cases, unverified
user-statement evidence for uncertain/negative cases, exact existing records for
duplicates, different existing conclusions for conflicts and revisions, and
the named mutation for malformed/sensitive cases. Synthetic receipts establish
the executable policy contract, not the real-world truth of fixture sentences.

The negative scenarios deliberately expect `pending_review` if submitted with
weak evidence: the store does not decide whether greetings or progress have
future value. Agent guidance must suppress those submissions. Conflating that
expectation with program rejection would conceal an acceptance boundary.

All successful/error branches check storage effects; accepted results check
exact retry receipts, revisions preserve the original Active content, and
sensitive rejection checks that the submitted body is absent from the
checkpointed database. The existing CLI bilingual classifier tests separately
cover the cheap reminder filter; a reminder is not a memory-value decision.

Run the program-policy corpus with:

```sh
cargo test -p art-agent-store --test unified_intake_contracts bilingual_corpus -- --nocapture
```
