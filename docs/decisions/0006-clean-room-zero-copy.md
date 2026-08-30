# ADR-0006: independent clean-room implementation

Status: accepted for ART 0.1.

## Requirement

ART must be an independently reasoned product whose identity, data model, workflows, tests, and language follow its own user and ecosystem constraints.

## Decision

Core choices are derived from ART requirements: per-Agent physical ownership, separate MemoryArtifact/SourceAnchor/AssuranceDecision, new human-owned Knowledge Editions, temporary Recall Bundles, and process-bound stdio tools. External product source, schemas, prompts, tests, storage layouts, and proprietary workflow combinations are prohibited inputs.

## Rejected alternatives

Mechanical reimplementation, translation, renaming, or compatibility with another memory product was rejected regardless of source-license permission.

## Consequences

Release runs terminology, dependency, structure, secret, and provenance checks. Only general-purpose libraries and public standards are implementation dependencies.
