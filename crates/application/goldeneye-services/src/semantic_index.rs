use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::GraphNode;
use goldeneye_ports::{NodeSignatureRecord, NodeVectorRecord, StoredVector, TokenVectorRecord};

use crate::{Generation, IndexRepositoryMode, ProjectId, ServiceError, Services};

type SemanticDocuments = Vec<(GraphNode, Vec<String>)>;
type SemanticTokens = BTreeMap<String, (f32, goldeneye_query::SemanticVector)>;

impl Services {
    pub(crate) fn refresh_semantic_index_at(
        &self,
        project: &ProjectId,
        expected_generation: Generation,
        mode: IndexRepositoryMode,
    ) -> Result<(), ServiceError> {
        if mode == IndexRepositoryMode::Fast {
            return self.replace_semantic_index_at(project, expected_generation, &[], &[], &[]);
        }

        let nodes = self.load_semantic_nodes(project)?;
        let model = goldeneye_query::PretrainedModel::load_bundled().ok();
        let (documents, document_frequency) = collect_semantic_documents(nodes);
        let (token_vectors, semantic_tokens) =
            semantic_token_index(documents.len(), &document_frequency, model.as_ref());
        let (node_vectors, signatures) = semantic_node_index(documents, &semantic_tokens);
        self.replace_semantic_index_at(
            project,
            expected_generation,
            &node_vectors,
            &token_vectors,
            &signatures,
        )
    }

    fn load_semantic_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, ServiceError> {
        let query = self
            .dependencies
            .repositories()
            .open_query(&self.config.database_path)
            .map_err(ServiceError::Repository)?;
        let nodes = query
            .list_nodes(project)
            .map_err(ServiceError::Repository)?;
        drop(query);
        Ok(nodes)
    }

    fn replace_semantic_index_at(
        &self,
        project: &ProjectId,
        expected_generation: Generation,
        node_vectors: &[NodeVectorRecord],
        token_vectors: &[TokenVectorRecord],
        signatures: &[NodeSignatureRecord],
    ) -> Result<(), ServiceError> {
        self.dependencies
            .repositories()
            .open_semantic_index(&self.config.database_path)
            .map_err(ServiceError::Repository)?
            .replace_semantic_index(
                project,
                expected_generation,
                node_vectors,
                token_vectors,
                signatures,
            )
            .map_err(ServiceError::Repository)
    }
}

fn collect_semantic_documents(
    nodes: Vec<GraphNode>,
) -> (SemanticDocuments, BTreeMap<String, usize>) {
    let mut documents = Vec::new();
    let mut document_frequency = BTreeMap::<String, usize>::new();
    for node in nodes
        .into_iter()
        .filter(|node| {
            !matches!(
                node.label.as_str(),
                "File" | "Folder" | "Module" | "Package" | "EnvVar"
            )
        })
        .take(goldeneye_query::SEMANTIC_MAX_OCCURRENCES)
    {
        let text = semantic_document(&node);
        let tokens =
            goldeneye_query::tokenize_identifier(&text, goldeneye_query::MAX_STRUCTURAL_TOKENS);
        if tokens.is_empty() {
            continue;
        }
        for token in tokens.iter().map(String::as_str).collect::<BTreeSet<_>>() {
            *document_frequency.entry(token.to_owned()).or_default() += 1;
        }
        documents.push((node, tokens));
    }
    (documents, document_frequency)
}

fn semantic_token_index(
    document_count: usize,
    document_frequency: &BTreeMap<String, usize>,
    model: Option<&goldeneye_query::PretrainedModel>,
) -> (Vec<TokenVectorRecord>, SemanticTokens) {
    let mut token_vectors = Vec::with_capacity(document_frequency.len());
    let mut semantic_tokens = BTreeMap::new();
    for (token, frequency) in document_frequency {
        // Index limits keep both operands well below f32's exact integer range.
        #[allow(clippy::cast_precision_loss)]
        let idf = (((document_count + 1) as f32 / (*frequency + 1) as f32).ln() + 1.0).max(0.0);
        let mut vector = goldeneye_query::SemanticVector::for_token(token, model);
        vector.normalize();
        // IDF is non-negative and logarithmic; milli-units remain far below u32::MAX.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let idf_milli = (idf * 1_000.0).round().max(0.0) as u32;
        let quantized_vector = quantize_semantic(vector.values());
        semantic_tokens.insert(token.clone(), (idf, vector));
        token_vectors.push(TokenVectorRecord {
            token: token.clone(),
            vector: quantized_vector,
            idf_milli,
        });
    }
    (token_vectors, semantic_tokens)
}

fn semantic_node_index(
    documents: SemanticDocuments,
    semantic_tokens: &SemanticTokens,
) -> (Vec<NodeVectorRecord>, Vec<NodeSignatureRecord>) {
    let mut node_vectors = Vec::with_capacity(documents.len());
    let mut signatures = Vec::new();
    for (node, tokens) in documents {
        let mut vector = goldeneye_query::SemanticVector::zero();
        for token in &tokens {
            if let Some((weight, token_vector)) = semantic_tokens.get(token) {
                vector.add_scaled(token_vector, *weight);
            }
        }
        vector.normalize();
        node_vectors.push(NodeVectorRecord {
            node_id: node.id.clone(),
            vector: quantize_semantic(vector.values()),
        });
        if let Some(signature) = semantic_signature(node, &tokens) {
            signatures.push(signature);
        }
    }
    (node_vectors, signatures)
}

fn semantic_signature(node: GraphNode, tokens: &[String]) -> Option<NodeSignatureRecord> {
    let token_refs = tokens.iter().map(String::as_str).collect::<Vec<_>>();
    let signature = goldeneye_query::MinHashSignature::from_normalized_tokens(&token_refs)?;
    let ast_profile = node.properties.get("ast_profile").map(|value| {
        value
            .as_str()
            .map_or_else(|| value.to_string(), ToOwned::to_owned)
    });
    Some(NodeSignatureRecord {
        node_id: node.id,
        minhash_hex: signature.to_hex(),
        ast_profile,
    })
}

fn semantic_document(node: &GraphNode) -> String {
    let mut document = format!(
        "{} {} {}",
        node.label.as_str(),
        node.name,
        node.qualified_name.as_str()
    );
    for key in [
        "signature",
        "docstring",
        "decorators",
        "return_type",
        "ast_profile",
    ] {
        if let Some(value) = node.properties.get(key) {
            document.push(' ');
            if let Some(value) = value.as_str() {
                document.push_str(value);
            } else {
                document.push_str(&value.to_string());
            }
        }
    }
    document
}

#[allow(clippy::cast_possible_truncation)]
fn quantize_semantic(values: &[f32; goldeneye_query::SEMANTIC_DIM]) -> StoredVector {
    StoredVector::from_array(std::array::from_fn(|index| {
        (values[index] * 127.0).clamp(-127.0, 127.0).trunc() as i8
    }))
}
