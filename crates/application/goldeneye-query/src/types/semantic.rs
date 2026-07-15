use goldeneye_domain::ProjectId;
use serde::{Deserialize, Serialize};

use super::NodeSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticSearchRequest {
    pub project: ProjectId,
    pub keywords: Vec<String>,
    pub limit: usize,
}

impl SemanticSearchRequest {
    #[must_use]
    pub const fn new(project: ProjectId, keywords: Vec<String>) -> Self {
        Self {
            project,
            keywords,
            limit: 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticSearchHit {
    pub node: NodeSummary,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    pub project: String,
    pub keyword_count: usize,
    pub results: Vec<SemanticSearchHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilaritySearchRequest {
    pub project: ProjectId,
    pub qualified_name: String,
    pub threshold: f64,
    pub limit: usize,
}

impl SimilaritySearchRequest {
    #[must_use]
    pub fn new(project: ProjectId, qualified_name: impl Into<String>) -> Self {
        Self {
            project,
            qualified_name: qualified_name.into(),
            threshold: 0.95,
            limit: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilaritySearchHit {
    pub node: NodeSummary,
    pub similarity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilaritySearchResult {
    pub project: String,
    pub origin: NodeSummary,
    pub results: Vec<SimilaritySearchHit>,
}
