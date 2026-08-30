# Independent design review

ART is derived from its own requirements: private continuity for one bound coding Agent, evidence-aware correction, deliberate conversion of stable conclusions into reviewed shared knowledge, and local operation through stdio MCP.

The resulting architecture is independently constrained:

- private experience is physically separated by Agent identity rather than represented as visibility flags in a shared pool;
- Memory Artifact, Source Anchor, and Assurance Decision have separate lifecycles;
- shared Knowledge Edition is a newly reviewed object, not a promoted memory status;
- recall first applies identity, state, validity, sensitivity, and grant admission, then ranks private and shared lanes separately;
- shared files contain commitments and review receipts, while private source locks stay in an owner-only control store;
- cross-Agent transport remains an optional, expiring, no-persist grant contract and never becomes a second memory authority;
- publication and lifecycle recovery explicitly model the SQLite/filesystem boundary instead of claiming false atomicity.

Rejected convergence patterns include a project-wide private-memory pool, automatic memory-to-knowledge promotion, Agent approval/publication, transparent embedding dependence, repository-wide transcript collection, host orchestration, dashboards, and network daemons. No external product source, schema, prompt, fixture, directory layout, or workflow text is used in implementation or tests.
