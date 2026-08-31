use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// User-selected retrieval strategy. Semantic modes remain optional and must
/// degrade to lexical retrieval when no healthy provider is configured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    #[default]
    Lexical,
    FullScan,
    Semantic,
    Hybrid,
}

/// Controls whether recall returns a compact route map or ranked recall items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecallDetail {
    Route,
    #[default]
    Recall,
}
