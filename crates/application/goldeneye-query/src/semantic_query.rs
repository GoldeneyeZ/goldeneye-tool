// Quantized ranking converts bounded persisted integer fields to floating-point scores.
#![allow(clippy::cast_precision_loss)]

use std::collections::BTreeMap;

use goldeneye_domain::{GraphNode, NodeId};
use goldeneye_ports::{NodeVectorRecord, QueryRepository, StoredVector};

use crate::{
    MinHashSignature, SemanticVector,
    engine::{ResolveMode, degrees, node_summary, resolve_symbol},
    types::{
        QueryError, SemanticSearchHit, SemanticSearchRequest, SemanticSearchResult,
        SimilaritySearchHit, SimilaritySearchRequest, SimilaritySearchResult,
    },
};

const MAX_KEYWORDS: usize = 32;
const DEFAULT_SEMANTIC_LIMIT: usize = 16;
const MAX_SEMANTIC_LIMIT: usize = 200;
const MAX_SIMILARITY_LIMIT: usize = 200;

struct SemanticSearchInput<'a> {
    keywords: Vec<&'a String>,
    limit: usize,
}

struct SemanticCandidate {
    record: NodeVectorRecord,
    node: GraphNode,
    initial_score: f64,
}

pub(crate) fn semantic_search(
    repository: &dyn QueryRepository,
    request: &SemanticSearchRequest,
) -> Result<SemanticSearchResult, QueryError> {
    require_project(repository, &request.project)?;
    let input = validate_semantic_search(request)?;
    let keyword_vectors = input
        .keywords
        .iter()
        .map(|keyword| keyword_vector(repository, &request.project, keyword))
        .collect::<Result<Vec<_>, _>>()?;
    let results = rank_semantic_hits(repository, request, &keyword_vectors, input.limit)?;

    Ok(SemanticSearchResult {
        project: request.project.as_str().to_owned(),
        keyword_count: input.keywords.len(),
        results,
    })
}

fn validate_semantic_search(
    request: &SemanticSearchRequest,
) -> Result<SemanticSearchInput<'_>, QueryError> {
    let keywords = request
        .keywords
        .iter()
        .filter(|keyword| !keyword.is_empty())
        .take(MAX_KEYWORDS)
        .collect::<Vec<_>>();
    if keywords.is_empty() {
        return Err(QueryError::InvalidSemanticKeywords {
            maximum: MAX_KEYWORDS,
        });
    }
    let limit = if request.limit == 0 {
        DEFAULT_SEMANTIC_LIMIT
    } else {
        request.limit
    };
    if limit > MAX_SEMANTIC_LIMIT {
        return Err(QueryError::InvalidSemanticLimit {
            actual: request.limit,
            maximum: MAX_SEMANTIC_LIMIT,
        });
    }
    Ok(SemanticSearchInput { keywords, limit })
}

fn rank_semantic_hits(
    repository: &dyn QueryRepository,
    request: &SemanticSearchRequest,
    keyword_vectors: &[StoredVector],
    limit: usize,
) -> Result<Vec<SemanticSearchHit>, QueryError> {
    let nodes = repository.list_nodes(&request.project)?;
    let nodes_by_id: BTreeMap<NodeId, GraphNode> = nodes
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect();
    let edges = repository.list_edges(&request.project)?;
    let node_degrees = degrees(&edges);
    let node_vectors = repository.list_node_vectors(&request.project)?;
    let candidates =
        initial_semantic_candidates(node_vectors, &nodes_by_id, &keyword_vectors[0], limit);
    Ok(final_semantic_hits(
        candidates,
        &keyword_vectors[1..],
        &node_degrees,
        limit,
    ))
}

fn initial_semantic_candidates(
    node_vectors: Vec<NodeVectorRecord>,
    nodes_by_id: &BTreeMap<NodeId, GraphNode>,
    keyword_vector: &StoredVector,
    limit: usize,
) -> Vec<SemanticCandidate> {
    let mut candidates = node_vectors
        .into_iter()
        .filter_map(|record| {
            let node = nodes_by_id.get(&record.node_id)?;
            matches!(node.label.as_str(), "Function" | "Method" | "Class")
                .then_some((record, node.clone()))
        })
        .map(|(record, node)| {
            let initial_score = quantized_cosine(&record.vector, keyword_vector);
            SemanticCandidate {
                record,
                node,
                initial_score,
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .initial_score
            .total_cmp(&left.initial_score)
            .then_with(|| left.node.id.cmp(&right.node.id))
    });
    candidates.truncate(limit.saturating_mul(5));
    candidates
}

fn final_semantic_hits(
    candidates: Vec<SemanticCandidate>,
    remaining_keywords: &[StoredVector],
    node_degrees: &BTreeMap<NodeId, (usize, usize)>,
    limit: usize,
) -> Vec<SemanticSearchHit> {
    let mut results = candidates
        .into_iter()
        .map(|candidate| {
            let combined_score = remaining_keywords
                .iter()
                .map(|keyword| quantized_cosine(&candidate.record.vector, keyword))
                .fold(candidate.initial_score, f64::min);
            SemanticSearchHit {
                node: node_summary(&candidate.node, None, node_degrees, Vec::new()),
                score: combined_score,
            }
        })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.node.id.cmp(&right.node.id))
    });
    results.truncate(limit);
    results
}

pub(crate) fn similarity_search(
    repository: &dyn QueryRepository,
    request: &SimilaritySearchRequest,
) -> Result<SimilaritySearchResult, QueryError> {
    require_project(repository, &request.project)?;
    if !(request.threshold > 0.0 && request.threshold <= 1.0) {
        return Err(QueryError::InvalidSimilarityThreshold {
            actual: request.threshold,
        });
    }
    if request.limit == 0 || request.limit > MAX_SIMILARITY_LIMIT {
        return Err(QueryError::InvalidSimilarityLimit {
            actual: request.limit,
            maximum: MAX_SIMILARITY_LIMIT,
        });
    }

    let nodes = repository.list_nodes(&request.project)?;
    let edges = repository.list_edges(&request.project)?;
    let node_degrees = degrees(&edges);
    let origin = resolve_symbol(
        &request.qualified_name,
        &nodes,
        &node_degrees,
        ResolveMode::Any,
    )?;
    let origin_record = repository
        .get_node_signature(&request.project, &origin.id)?
        .ok_or_else(|| QueryError::SignatureNotFound {
            qualified_name: origin.qualified_name.as_str().to_owned(),
        })?;
    let origin_signature = decode_signature(&origin_record.minhash_hex, &origin.id)?;
    let nodes_by_id: BTreeMap<NodeId, GraphNode> = nodes
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect();

    let mut results = Vec::new();
    for record in repository.list_node_signatures(&request.project)? {
        if record.node_id == origin.id {
            continue;
        }
        let signature = decode_signature(&record.minhash_hex, &record.node_id)?;
        let similarity = origin_signature.similarity(&signature);
        if similarity < request.threshold {
            continue;
        }
        let Some(node) = nodes_by_id.get(&record.node_id) else {
            return Err(QueryError::CorruptSemanticArtifact {
                reason: format!("signature references missing node {:?}", record.node_id),
            });
        };
        results.push(SimilaritySearchHit {
            node: node_summary(node, None, &node_degrees, Vec::new()),
            similarity,
        });
    }
    results.sort_by(|left, right| {
        right
            .similarity
            .total_cmp(&left.similarity)
            .then_with(|| left.node.qualified_name.cmp(&right.node.qualified_name))
            .then_with(|| left.node.id.cmp(&right.node.id))
    });
    results.truncate(request.limit);

    Ok(SimilaritySearchResult {
        project: request.project.as_str().to_owned(),
        origin: node_summary(&origin, None, &node_degrees, Vec::new()),
        results,
    })
}

fn require_project(
    repository: &dyn QueryRepository,
    project: &goldeneye_domain::ProjectId,
) -> Result<(), QueryError> {
    repository
        .get_project(project)?
        .map(|_| ())
        .ok_or_else(|| QueryError::ProjectNotFound(project.clone()))
}

fn keyword_vector(
    repository: &dyn QueryRepository,
    project: &goldeneye_domain::ProjectId,
    keyword: &str,
) -> Result<StoredVector, QueryError> {
    if let Some(record) = repository.get_token_vector(project, keyword)? {
        return Ok(record.vector);
    }
    let mut vector = SemanticVector::sparse_random_index(keyword);
    vector.normalize();
    Ok(quantize(vector.values()))
}

#[allow(clippy::cast_possible_truncation)]
fn quantize(values: &[f32; 768]) -> StoredVector {
    let quantized = std::array::from_fn(|index| {
        let scaled = (values[index] * 127.0).clamp(-127.0, 127.0);
        scaled.trunc() as i8
    });
    StoredVector::from_array(quantized)
}

fn quantized_cosine(left: &StoredVector, right: &StoredVector) -> f64 {
    let mut dot = 0_i64;
    let mut left_magnitude = 0_i64;
    let mut right_magnitude = 0_i64;
    for (&left, &right) in left.values().iter().zip(right.values()) {
        let left = i64::from(left);
        let right = i64::from(right);
        dot += left * right;
        left_magnitude += left * left;
        right_magnitude += right * right;
    }
    if left_magnitude == 0 || right_magnitude == 0 {
        return 0.0;
    }
    dot as f64 / ((left_magnitude as f64) * (right_magnitude as f64)).sqrt()
}

fn decode_signature(encoded: &str, node: &NodeId) -> Result<MinHashSignature, QueryError> {
    MinHashSignature::from_hex(encoded).map_err(|error| QueryError::CorruptSemanticArtifact {
        reason: format!("invalid signature for {node:?}: {error}"),
    })
}
