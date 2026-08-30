# ADR-0003: immutable Knowledge Editions and lifecycle events

Status: accepted for ART 0.1.

## Requirement

Published knowledge must remain auditable across correction, replacement, crash recovery, and projection loss.

## Decision

Every publication creates new Markdown and manifest files at an immutable path. Revocation and supersession create new hashed event files. A recoverable intent protocol bridges filesystem and SQLite boundaries; current, revoked, and search state are rebuildable projections.

## Rejected alternatives

Editing a published file in place and treating a SQLite current row as the sole authority were rejected because both erase history and make recovery ambiguous.

## Consequences

ART never stages, commits, pushes, or switches Git state. Human owners retain control of repository publication.
