#![allow(clippy::float_cmp)]

mod common;

use common::Fixture;
use goldeneye_query::{MinHashSignature, SemanticSearchRequest, SimilaritySearchRequest};
use goldeneye_store::{
    NodeSignatureRecord, NodeVectorRecord, Store, StoredVector, TokenVectorRecord,
};

fn vector(entries: &[(usize, i8)]) -> StoredVector {
    let mut values = [0_i8; 768];
    for &(index, value) in entries {
        values[index] = value;
    }
    StoredVector::from_array(values)
}

#[test]
fn semantic_search_uses_enriched_tokens_and_the_minimum_multi_keyword_cosine() {
    let fixture = Fixture::seeded();
    let mut store = Store::open(&fixture.database).expect("store");
    store
        .replace_semantic_index(
            &fixture.project,
            &[
                NodeVectorRecord {
                    node_id: goldeneye_domain::NodeId::new("alpha").expect("node"),
                    vector: vector(&[(0, 90), (1, 90)]),
                },
                NodeVectorRecord {
                    node_id: goldeneye_domain::NodeId::new("beta").expect("node"),
                    vector: vector(&[(0, 127)]),
                },
                NodeVectorRecord {
                    node_id: goldeneye_domain::NodeId::new("free-run").expect("node"),
                    vector: vector(&[(1, 127)]),
                },
            ],
            &[
                TokenVectorRecord {
                    token: "find".to_owned(),
                    vector: vector(&[(0, 127)]),
                    idf_milli: 1_000,
                },
                TokenVectorRecord {
                    token: "user".to_owned(),
                    vector: vector(&[(1, 127)]),
                    idf_milli: 1_000,
                },
            ],
            &[],
        )
        .expect("semantic snapshot");
    drop(store);

    let engine = fixture.engine();
    let mut request = SemanticSearchRequest::new(
        fixture.project.clone(),
        vec!["find".to_owned(), "user".to_owned()],
    );
    request.limit = 3;
    let result = engine.semantic_search(&request).expect("semantic search");

    assert_eq!(result.keyword_count, 2);
    assert_eq!(result.results[0].node.id, "alpha");
    assert!(result.results[0].score > 0.70);
    assert_eq!(result.results[1].score, 0.0);
    assert_eq!(result.results[2].score, 0.0);
}

#[test]
fn similarity_search_decodes_persisted_minhash_and_excludes_the_origin() {
    let fixture = Fixture::seeded();
    let mut store = Store::open(&fixture.database).expect("store");
    let same = MinHashSignature::from_values([7_u32; 64]).to_hex();
    let different = MinHashSignature::from_values([9_u32; 64]).to_hex();
    store
        .replace_semantic_index(
            &fixture.project,
            &[],
            &[],
            &[
                NodeSignatureRecord {
                    node_id: goldeneye_domain::NodeId::new("alpha").expect("node"),
                    minhash_hex: same.clone(),
                    ast_profile: None,
                },
                NodeSignatureRecord {
                    node_id: goldeneye_domain::NodeId::new("beta").expect("node"),
                    minhash_hex: same,
                    ast_profile: None,
                },
                NodeSignatureRecord {
                    node_id: goldeneye_domain::NodeId::new("free-run").expect("node"),
                    minhash_hex: different,
                    ast_profile: None,
                },
            ],
        )
        .expect("signature snapshot");
    drop(store);

    let engine = fixture.engine();
    let request = SimilaritySearchRequest::new(fixture.project.clone(), "demo.src.lib.Alpha");
    let result = engine
        .similarity_search(&request)
        .expect("similarity search");

    assert_eq!(result.origin.id, "alpha");
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].node.id, "beta");
    assert_eq!(result.results[0].similarity, 1.0);
}
