# ADR-0005: optional thin cross-Agent contracts

Status: accepted for ART 0.1.

## Requirement

ART must work standalone for Codex and DSH, while leaving a governed future path for narrowly scoped cross-Agent delivery.

## Decision

The optional contract surface defines purpose-bound grants, short-lived Context Packs, recall references, no-persist provenance, and invalidation epochs. It contains no API for copying or writing another Agent Vault.

## Rejected alternatives

A second shared memory authority and mandatory coordinator dependency were rejected because they duplicate ART ownership and break standalone operation.

## Consequences

Reviewed Knowledge Editions are the default multi-Agent sharing mechanism. Private excerpts require explicit bounded authorization outside the ART 0.1 runtime.
