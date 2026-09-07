# Security model

## Protected assets

ART protects private Agent experience, source locations and excerpts, knowledge review records, commitment material, and the boundary between Agent and human authority.

## Controls

- Physical per-Agent SQLite files and startup identity binding prevent cross-Agent access through ART interfaces.
- Private files use owner-only permissions; directories use owner-only traversal.
- MCP exposes no SQL, owner selector, deletion, grant, or cross-Agent tool.
- The governance page binds only to a random `127.0.0.1` port. Short-lived
  high-entropy session capabilities, exact Origin checks, CSRF tokens, Agent
  binding, host binding, proposal revision binding, and bounded redacted JSON
  protect every page mutation.
- Delegation is default-off and persists only for one host binding plus Agent
  identity. It is not inferred from Full Access. Delegated approval and
  publication are recorded as `AgentDelegated`, never Human, with the fixed
  `current_user_instruction` authorization basis.
- Structured anchors reject common credential forms, private keys, authorization headers, raw transcripts, unsafe receipt shapes, oversized source versions/digests, and forged export hashes.
- Publication shares keyed commitments and hashes instead of private source identifiers.
- Paths are canonicalized and constrained; import/export rejects symbolic links, hard links, unsafe content, and existing targets.
- Tool output is object-shaped and has stable schemas for strict MCP clients.
- Debug logging is clamped to ART targets; stdio remains JSON-RPC only and request bodies are not echoed to stderr.
- SQLite busy, read-only, simulated disk-full, migration-race, WAL, corrupted projection, and partial-publication behavior is fail-closed and tested.
- Optional semantic transport is outbound HTTPS only, rejects redirects and URL credentials, bounds request/response sizes, reads an optional owner-only token at request time, and reports only safe provider/projection status. Vector rebuild sends only private memory that is Active and validity-eligible at collection time; Candidate, future-valid, expired, disputed, superseded, and archived bodies remain local.

## Prompt injection

Memory and knowledge are data, not instructions. Hosts must not elevate permissions because stored text requests it. Knowledge review must treat embedded tool directions, policy overrides, and credential requests as suspicious. ART does not bypass the host's normal approval and sandbox policy.

## Delegated trust boundary

Delegated mode deliberately weakens the human-review boundary: ART trusts that
the bound Agent correctly recognized an unambiguous current user instruction.
It does not authenticate the speaker or retain the raw prompt. The benefit is
one-instruction governance; the compensating controls are explicit persistent
opt-in, identity scoping, atomic publication, exact source/snapshot checks,
distinct audit labels, and immediate disable from the local page.

## Limits

File permissions are not isolation from arbitrary code running as the same operating-system user. A Codex/DSH process with unrestricted filesystem capability can bypass the ART application boundary; use separate OS identities or an external sandbox for hostile Agents. ART does not encrypt live data at rest, scan every possible secret encoding, authenticate an embedding provider beyond TLS and an optional token, or provide remote multi-tenant isolation in v0.3.0. Enabling semantic retrieval sends bounded query/document text to the operator-selected endpoint and is therefore a separate disclosure decision.

The Git backup contains only reviewed immutable knowledge and lifecycle events.
The non-rebuildable Control Store and commitment key enter Git only inside an
age-encrypted recovery capsule bound to the knowledge tree hash. Agent Vaults,
queries, Recall Bundles, source excerpts, plaintext keys, credentials, and
SQLite WAL/SHM files, navigation indexes, and semantic projections are excluded. ART never reads an SSH private key; the
operator supplies an age/SSH recipient for backup and an identity only during
an explicit restore.

## Reporting

Follow `SECURITY.md`. Never include live credentials or private memory bodies in a report.
