# ADR-0001: one private Vault per Agent

Status: accepted for ART 0.1.

## Requirement

Private recall must belong to the Agent that formed it, and an Agent-bound MCP process must not enumerate or read another Agent's memory.

## Decision

Each Agent has a physically separate SQLite file. The path and database metadata are both bound to the canonical Agent ID at open time. MCP identity is fixed by process arguments and is absent from tool input schemas.

## Rejected alternatives

A shared private database with visibility flags was rejected because an omitted filter could cross the primary privacy boundary. Per-session databases were rejected because they would not provide Agent continuity.

## Consequences

Human operator commands may explicitly select a Vault. Strong isolation from arbitrary filesystem access by another process running as the same OS user remains a deployment responsibility.
