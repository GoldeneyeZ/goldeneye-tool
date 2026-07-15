// Quantized ranking converts bounded persisted integer fields to floating-point scores.
#![allow(clippy::cast_precision_loss)]

use std::collections::BTreeMap;

use goldeneye_domain::{GraphNode, NodeId};
use goldeneye_store::{QueryStore, StoredVector};

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

pub(crate) fn semantic_search(
    store: &QueryStore,
    request: &SemanticSearchRequest,
) -> Result<SemanticSearchResult, QueryError> {
    require_project(store, &request.project)?;
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

    let keyword_vectors = keywords
        .iter()
        .map(|keyword| keyword_vector(store, &request.project, keyword))
        .collect::<Result<Vec<_>, _>>()?;
    let nodes = store.list_nodes(&request.project)?;
    let nodes_by_id: BTreeMap<NodeId, GraphNode> = nodes
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect();
    let edges = store.list_edges(&request.project)?;
    let node_degrees = degrees(&edges);

    let mut candidates = store
        .list_node_vectors(&request.project)?
        .into_iter()
        .filter_map(|record| {
            let node = nodes_by_id.get(&record.node_id)?;
            matches!(node.label.as_str(), "Function" | "Method" | "Class")
                .then_some((record, node.clone()))
        })
        .map(|(record, node)| {
            let first_score = quantized_cosine(&record.vector, &keyword_vectors[0]);
            (record, node, first_score)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    candidates.truncate(limit.saturating_mul(5));

    let mut results = candidates
        .into_iter()
        .map(|(record, node, initial_score)| {
            let combined_score = keyword_vectors[1..]
                .iter()
                .map(|keyword| quantized_cosine(&record.vector, keyword))
                .fold(initial_score, f64::min);
            SemanticSearchHit {
                node: node_summary(&node, None, &node_degrees, Vec::new()),
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

    Ok(SemanticSearchResult {
        project: request.project.as_str().to_owned(),
        keyword_count: keywords.len(),
        results,
    })
}

pub(crate) fn similarity_search(
    store: &QueryStore,
    request: &SimilaritySearchRequest,
) -> Result<SimilaritySearchResult, QueryError> {
    require_project(store, &request.project)?;
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

    let nodes = store.list_nodes(&request.project)?;
    let edges = store.list_edges(&request.project)?;
    let node_degrees = degrees(&edges);
    let origin = resolve_symbol(
        &request.qualified_name,
        &nodes,
        &node_degrees,
        ResolveMode::Any,
    )?;
    let origin_record = store
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
    for record in store.list_node_signatures(&request.project)? {
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
    store: &QueryStore,
    project: &goldeneye_domain::ProjectId,
) -> Result<(), QueryError> {
    store
        .get_project(project)?
        .map(|_| ())
        .ok_or_else(|| QueryError::ProjectNotFound(project.clone()))
}

fn keyword_vector(
    store: &QueryStore,
    project: &goldeneye_domain::ProjectId,
    keyword: &str,
) -> Result<StoredVector, QueryError> {
    if let Some(record) = store.get_token_vector(project, keyword)? {
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
