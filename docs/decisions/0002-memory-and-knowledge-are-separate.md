# ADR-0002: memory and knowledge are separate object families

Status: accepted for ART 0.1.

## Requirement

Agents need private continuity, while multi-Agent reuse requires review, redaction, stable publication, and human ownership.

## Decision

MemoryArtifact remains in one Agent Vault. A Knowledge Proposal locks exact sources, and human review may create a new Knowledge Edition. Publication never changes the source memory's owner or lifecycle state.

## Rejected alternatives

A single object whose status changes from private memory to shared knowledge was rejected because it conflates ownership, assurance, visibility, and publication.

## Consequences

Source changes can stale a proposal without rewriting memory. Shared manifests use commitments and omit private Agent and memory identifiers.
