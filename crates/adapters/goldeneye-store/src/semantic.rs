use super::{
    BTreeSet, Connection, Generation, MINHASH_SIGNATURE_HEX_LEN, NodeId, NodeSignatureRecord,
    NodeVectorRecord, OptionalExtension, ProjectId, STORED_VECTOR_DIM, SemanticIndexOutcome, Store,
    StoreError, StoredVector, TokenVectorRecord, Transaction, TransactionBehavior, corrupt_graph,
    ensure_generation, ensure_project_exists, params, sqlite_u64,
};

impl Store {
    /// Atomically replaces all persisted semantic vectors and structural signatures.
    ///
    /// # Errors
    ///
    /// Returns a validation, project-not-found, foreign-key, overflow, or storage error.
    pub fn replace_semantic_index(
        &mut self,
        project: &ProjectId,
        expected_generation: Generation,
        node_vectors: &[NodeVectorRecord],
        token_vectors: &[TokenVectorRecord],
        node_signatures: &[NodeSignatureRecord],
    ) -> Result<SemanticIndexOutcome, StoreError> {
        validate_semantic_index(node_vectors, token_vectors, node_signatures)?;
        ensure_project_exists(&self.connection, project)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_generation(&transaction, project, expected_generation)?;
        delete_semantic_index(&transaction, project)?;
        insert_node_vectors(&transaction, project, node_vectors)?;
        insert_token_vectors(&transaction, project, token_vectors)?;
        insert_node_signatures(&transaction, project, node_signatures)?;
        transaction.commit()?;
        Ok(SemanticIndexOutcome {
            node_vectors: node_vectors.len(),
            token_vectors: token_vectors.len(),
            node_signatures: node_signatures.len(),
        })
    }
}

pub(super) fn validate_semantic_index(
    node_vectors: &[NodeVectorRecord],
    token_vectors: &[TokenVectorRecord],
    node_signatures: &[NodeSignatureRecord],
) -> Result<(), StoreError> {
    validate_node_vectors(node_vectors)?;
    validate_token_vectors(token_vectors)?;
    validate_node_signatures(node_signatures)
}

fn validate_node_vectors(node_vectors: &[NodeVectorRecord]) -> Result<(), StoreError> {
    let mut vector_nodes = BTreeSet::new();
    for record in node_vectors {
        if !vector_nodes.insert(&record.node_id) {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!("duplicate node vector for {:?}", record.node_id),
            });
        }
    }
    Ok(())
}

fn validate_token_vectors(token_vectors: &[TokenVectorRecord]) -> Result<(), StoreError> {
    let mut tokens = BTreeSet::new();
    for record in token_vectors {
        if record.token.is_empty() || record.token.contains('\0') {
            return Err(StoreError::InvalidSemanticRecord {
                reason: "token must be non-empty and contain no NUL bytes".to_owned(),
            });
        }
        if !tokens.insert(&record.token) {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!("duplicate token vector for {:?}", record.token),
            });
        }
    }
    Ok(())
}

fn validate_node_signatures(node_signatures: &[NodeSignatureRecord]) -> Result<(), StoreError> {
    let mut signature_nodes = BTreeSet::new();
    for record in node_signatures {
        if !signature_nodes.insert(&record.node_id) {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!("duplicate node signature for {:?}", record.node_id),
            });
        }
        if record.minhash_hex.len() != MINHASH_SIGNATURE_HEX_LEN
            || !record
                .minhash_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!(
                    "MinHash signature for {:?} must contain {MINHASH_SIGNATURE_HEX_LEN} hex digits",
                    record.node_id
                ),
            });
        }
        if record
            .ast_profile
            .as_ref()
            .is_some_and(|profile| profile.contains('\0'))
        {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!("AST profile for {:?} contains a NUL byte", record.node_id),
            });
        }
    }
    Ok(())
}

fn delete_semantic_index(
    transaction: &Transaction<'_>,
    project: &ProjectId,
) -> Result<(), StoreError> {
    transaction.execute(
        "DELETE FROM node_vectors WHERE project_id = ?1",
        params![project.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM token_vectors WHERE project_id = ?1",
        params![project.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM node_signatures WHERE project_id = ?1",
        params![project.as_str()],
    )?;
    Ok(())
}

fn insert_node_vectors(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    records: &[NodeVectorRecord],
) -> Result<(), StoreError> {
    let mut statement = transaction
        .prepare("INSERT INTO node_vectors(project_id, node_id, vector) VALUES (?1, ?2, ?3)")?;
    for record in records {
        statement.execute(params![
            project.as_str(),
            record.node_id.as_str(),
            stored_vector_to_blob(&record.vector),
        ])?;
    }
    Ok(())
}

fn insert_token_vectors(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    records: &[TokenVectorRecord],
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare(
        "INSERT INTO token_vectors(project_id, token, vector, idf_milli) \
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for record in records {
        statement.execute(params![
            project.as_str(),
            record.token,
            stored_vector_to_blob(&record.vector),
            i64::from(record.idf_milli),
        ])?;
    }
    Ok(())
}

fn insert_node_signatures(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    records: &[NodeSignatureRecord],
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare(
        "INSERT INTO node_signatures(\
           project_id, node_id, minhash_hex, ast_profile\
         ) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for record in records {
        statement.execute(params![
            project.as_str(),
            record.node_id.as_str(),
            record.minhash_hex,
            record.ast_profile,
        ])?;
    }
    Ok(())
}

pub(super) fn list_node_vectors(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<NodeVectorRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT node_id, vector FROM node_vectors \
         WHERE project_id = ?1 ORDER BY node_id COLLATE BINARY",
    )?;
    let rows = statement.query_map(params![project.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    rows.map(|row| {
        let (node_id, vector) = row?;
        Ok(NodeVectorRecord {
            node_id: NodeId::new(node_id).map_err(corrupt_graph("node vector ID"))?,
            vector: stored_vector_from_blob(vector, "node vector")?,
        })
    })
    .collect()
}

pub(super) fn get_node_vector(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Option<NodeVectorRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT node_id, vector FROM node_vectors \
             WHERE project_id = ?1 AND node_id = ?2",
            params![project.as_str(), node.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?;
    raw.map(|(node_id, vector)| {
        Ok(NodeVectorRecord {
            node_id: NodeId::new(node_id).map_err(corrupt_graph("node vector ID"))?,
            vector: stored_vector_from_blob(vector, "node vector")?,
        })
    })
    .transpose()
}

pub(super) fn get_token_vector(
    connection: &Connection,
    project: &ProjectId,
    token: &str,
) -> Result<Option<TokenVectorRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT token, vector, idf_milli FROM token_vectors \
             WHERE project_id = ?1 AND token = ?2",
            params![project.as_str(), token],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    raw.map(|(token, vector, idf_milli)| {
        Ok(TokenVectorRecord {
            token,
            vector: stored_vector_from_blob(vector, "token vector")?,
            idf_milli: u32::try_from(sqlite_u64("token vector IDF", idf_milli)?).map_err(|_| {
                StoreError::CorruptData {
                    field: "token vector IDF",
                    reason: format!("value {idf_milli} does not fit u32"),
                }
            })?,
        })
    })
    .transpose()
}

pub(super) fn list_node_signatures(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<NodeSignatureRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT node_id, minhash_hex, ast_profile FROM node_signatures \
         WHERE project_id = ?1 ORDER BY node_id COLLATE BINARY",
    )?;
    let rows = statement.query_map(params![project.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (node_id, minhash_hex, ast_profile) = row?;
        Ok(NodeSignatureRecord {
            node_id: NodeId::new(node_id).map_err(corrupt_graph("node signature ID"))?,
            minhash_hex,
            ast_profile,
        })
    })
    .collect()
}

pub(super) fn get_node_signature(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Option<NodeSignatureRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT node_id, minhash_hex, ast_profile FROM node_signatures \
             WHERE project_id = ?1 AND node_id = ?2",
            params![project.as_str(), node.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    raw.map(|(node_id, minhash_hex, ast_profile)| {
        Ok(NodeSignatureRecord {
            node_id: NodeId::new(node_id).map_err(corrupt_graph("node signature ID"))?,
            minhash_hex,
            ast_profile,
        })
    })
    .transpose()
}

pub(super) fn stored_vector_to_blob(vector: &StoredVector) -> Vec<u8> {
    vector
        .values()
        .iter()
        .map(|value| value.to_ne_bytes()[0])
        .collect()
}

pub(super) fn stored_vector_from_blob(
    blob: Vec<u8>,
    field: &'static str,
) -> Result<StoredVector, StoreError> {
    let bytes: [u8; STORED_VECTOR_DIM] =
        blob.try_into()
            .map_err(|value: Vec<u8>| StoreError::CorruptData {
                field,
                reason: format!("expected {STORED_VECTOR_DIM} bytes, found {}", value.len()),
            })?;
    Ok(StoredVector::from_array(
        bytes.map(|value| i8::from_ne_bytes([value])),
    ))
}
