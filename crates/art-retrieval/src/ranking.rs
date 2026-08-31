use art_domain::{ArtError, ArtResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankFusionPolicy {
    pub version: String,
    pub lexical_weight: f64,
    pub semantic_weight: f64,
    pub rrf_k: u32,
}

impl Default for RankFusionPolicy {
    fn default() -> Self {
        Self {
            version: "art.rank-fusion.v1".into(),
            lexical_weight: 1.0,
            semantic_weight: 0.7,
            rrf_k: 60,
        }
    }
}

impl RankFusionPolicy {
    pub fn load(path: &Path) -> ArtResult<Self> {
        validate_private_policy_file(path)?;
        let policy: Self = serde_json::from_slice(
            &fs::read(path)
                .map_err(|_| ArtError::Io("rank fusion policy file access failed".into()))?,
        )
        .map_err(|_| ArtError::InvalidInput("invalid rank fusion policy".into()))?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> ArtResult<()> {
        if self.version != "art.rank-fusion.v1"
            || !self.lexical_weight.is_finite()
            || !self.semantic_weight.is_finite()
            || self.lexical_weight < 0.0
            || self.semantic_weight < 0.0
            || (self.lexical_weight == 0.0 && self.semantic_weight == 0.0)
            || !(1..=1_000).contains(&self.rrf_k)
        {
            return Err(ArtError::InvalidInput("invalid rank fusion policy".into()));
        }
        Ok(())
    }
}

#[cfg(unix)]
fn validate_private_policy_file(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| ArtError::Io("rank fusion policy file access failed".into()))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(ArtError::PermissionDenied(
            "rank fusion policy must be one owner-only regular file".into(),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_private_policy_file(path: &Path) -> ArtResult<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| ArtError::Io("rank fusion policy file access failed".into()))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(ArtError::PermissionDenied(
            "rank fusion policy must be a regular file".into(),
        ));
    }
    Ok(())
}

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
use std::{fs, path::Path};
