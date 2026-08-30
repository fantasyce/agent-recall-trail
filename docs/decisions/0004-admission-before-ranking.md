# ADR-0004: admission before ranking

Status: accepted for ART 0.1.

## Requirement

Expired, disputed, revoked, wrong-identity, or unauthorized material must not become visible merely because it scores well.

## Decision

ART queries separate private and shared FTS5 projections, applies identity and lifecycle eligibility, then ranks bounded candidates with normalized exact matching, Jieba tokens, and CJK bigrams. Result lanes stay separate in Recall Bundle.

## Rejected alternatives

A global Top-K followed by filtering was rejected because it creates visibility and ranking side channels. A required embedding service was rejected because local deterministic Chinese recall is an ART 0.1 acceptance requirement.

## Consequences

Indexes are disposable and rebuildable. Recall Bundle is short-lived and carries a no-automatic-capture policy.
