# ART Hook objectivity adjustment acceptance

Date: 2026-09-18 (Asia/Shanghai). Scope: local candidate only; no public
release or formal Vault writes.

## Change

The Stop Hook continuation no longer says that ART "admitted" a completed
turn. It states that the cheap reminder filter is not evidence of value and
asks the Agent to evaluate the conclusion as if no Hook had fired. The Agent
must count evidence for and against recording without defaulting to either
outcome. Before submission it performs one bounded lexical recall and rejects
an existing conclusion in the same scope instead of submitting a paraphrase.

The deterministic ART checks remain unchanged. This prompt does not make model
judgment mathematically objective; it removes a known directional cue and adds
observable counter-checks.

## Test-driven contract

The new Hook-output contract was written first and failed against the former
"admitted this completed turn" wording. After the prompt change, all 11
`auto_memory_hook_contracts` tests pass. The contract verifies that the trigger
is described as non-evidence, evaluation is counterfactual and balanced, the
old directional wording is absent, and the bounded duplicate recall is
required.

## Real Codex evaluation

The evaluation used real Codex App Server threads with the locally built ART
binary, installed plugin Hook, persisted normal Hook trust, and a task-owned
Vault. The formal Vault and automatic-memory configuration were not connected
or changed.

Four adversarial negatives deliberately contained phrases such as `Decision`,
`verified`, and `tests passed`:

- transient local-variable progress;
- a true but trivial arithmetic statement;
- an unresolved failure with a speculative cause;
- a recalled conclusion with no new evidence.

Across repeated runs, every triggered negative continuation declined to call
`art_memory_candidate_submit`; the unresolved case was rejected by the cheap
filter before continuation. No negative became memory.

Two novel, file-digest-backed decisions were accepted and activated: a bounded
rollback procedure and a local-socket creation rule. Repeating those cases
exposed that paraphrased semantic duplicates could pass the exact backend
duplicate check. After adding the bounded lexical pre-submit recall, the Agent
found the existing socket rule and returned `no worth recording`; the Vault
count did not increase. A new, unrelated 14-day candidate-evidence retention
rule then passed the same recall step and activated, demonstrating that the
duplicate check does not suppress every positive.

Final prompt behavior covered six distinct cases:

| Case | Hook result | Memory result |
| --- | --- | --- |
| Transient progress | continued once, objectively rejected | none |
| Trivial truth | continued once, objectively rejected | none |
| Unresolved speculation | cheap filter rejected | none |
| Recalled text without new evidence | continued once, objectively rejected | none |
| Existing socket rule | recalled matching private memory | no new memory |
| Novel verified retention rule | recalled no duplicate, then submitted | Active |

The isolated test intentionally retained earlier duplicate examples so the
final duplicate test had adverse data to find. They are evaluation artifacts,
not formal user memories.

## Limitations

This is a bounded behavioral evaluation, not a proof that a probabilistic
model is objective. The sample is small and uses one installed model/version.
The result supports a narrower claim: the Hook no longer tells the Agent that
the turn has already qualified; tested misleading completion language was
rejected; verified novel content remained recordable; and a required recall
prevented another paraphrased duplicate in the observed run.
