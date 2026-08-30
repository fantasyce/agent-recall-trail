# Maintainer outreach

Subject: ART v0.2.0 — measured local retrieval for review-gated Agent knowledge

We have released ART v0.2.0, an Apache-2.0 local runtime that keeps each Agent's private memory separate and shares only human-reviewed Knowledge Editions. It is not a transcript store and does not let an Agent approve its own knowledge. The release adds BM25-first retrieval fusion, bounded result depth, and reproducible BEIR quality gates while preserving encrypted disaster recovery. We would value a design review of the retrieval, MCP, provenance, recovery, and operator boundaries before discussing any integration.
