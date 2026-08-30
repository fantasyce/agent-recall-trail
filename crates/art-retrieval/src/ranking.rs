pub(crate) fn rank_score(
    lexical_rank: usize,
    exact: bool,
    token_coverage: f64,
    bigram_coverage: f64,
) -> f64 {
    let rank = u32::try_from(lexical_rank).unwrap_or(u32::MAX);
    let base = 1.0 / (60.0 + f64::from(rank));
    let bounded_bonus = if exact { 0.000_20 } else { 0.0 }
        + token_coverage.clamp(0.0, 1.0) * 0.000_04
        + bigram_coverage.clamp(0.0, 1.0) * 0.000_02;
    base + bounded_bonus
}
