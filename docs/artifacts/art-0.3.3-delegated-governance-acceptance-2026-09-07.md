# ART 0.3.3 Delegated Governance Acceptance

**Status:** Candidate verification in progress  
**Branch:** `codex/art-delegated-governance-0.3.3`  
**Date:** 2026-09-07

## Candidate contract

ART 0.3.3 adds an ART-owned local governance page for Codex Desktop and DSH,
persistent default-off delegation scoped to one host binding plus Agent, one
atomic `approve_and_publish` operation, and linked `AgentDelegated` receipts.
Native MCP form Elicitation remains compatible for clients that support it.

The candidate has eight Agent-safe MCP tools. Agent input to delegated
governance remains limited to operation, Proposal ID, and exact revision.
Human-facing settings, review, publication, status, and audit use the local
page rather than a terminal workflow.

## Required evidence

- exact candidate commit and version consistency;
- formatting, warnings-as-errors analysis, workspace tests, and release gate;
- isolated installation and MCP tool discovery;
- random `127.0.0.1` page binding, capability, Origin, CSRF, and revision checks;
- delegation enable, restart persistence, atomic publication, distinct audit,
  immediate disable, and default-off behavior;
- Codex Desktop in-app browser journey;
- DSH page journey or an explicit environment limitation;
- residue, disk use, and final Git state.

Final results are appended only after each layer is exercised against the
candidate bytes.
