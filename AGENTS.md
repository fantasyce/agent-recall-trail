# ART development contract

- The current design and implementation plans under `docs/specs/` and `docs/plans/` are the product and acceptance contracts.
- Use test-driven development. Observe each new behavior test fail before implementing it.
- Keep every Agent Vault in a physically distinct SQLite file and bind MCP identity at process start.
- Memory and published knowledge are different object families. Agents never approve or publish knowledge.
- Do not read, copy, translate, or structurally imitate competitor source code, schemas, prompts, tests, names, or directory layouts.
- Tests use an explicit task-owned `ART_HOME`; never write test data to the formal `~/.across` runtime.
- Do not modify unrelated products, existing knowledge sources, or formal Codex/DSH configuration during automated tests.
- Do not persist secrets, full transcripts, unrestricted command output, or recalled bundle bodies.
