# Maintainer outreach

Subject: ART v0.3.0 — progressive recall for review-gated Agent knowledge

We have released ART v0.3.0, an Apache-2.0 local runtime that keeps each Agent's private memory separate and shares only human-reviewed Knowledge Editions. It is not a transcript store and does not let an Agent approve its own knowledge. This release adds route/recall/read, governed full scan, and optional provider-neutral semantic/hybrid retrieval while keeping lexical behavior stable on provider failure. We would value a design review of the retrieval policy, MCP, provenance, recovery, and operator boundaries before discussing any integration.
