use super::{
    Generation, GraphEdge, ProjectId, Store, StoreError, Transaction, TransactionBehavior,
    ensure_node_exists, insert_edge, params,
};

impl Store {
    /// Atomically replaces derived cross-project edges without rebuilding local graph rows.
    ///
    /// # Errors
    ///
    /// Returns a validation, project-not-found, missing-node, or storage error.
    pub fn replace_cross_project_edges(
        &mut self,
        project: &ProjectId,
        edges: &[GraphEdge],
    ) -> Result<usize, StoreError> {
        let generation = self
            .get_project(project)?
            .ok_or_else(|| StoreError::ProjectNotFound(project.clone()))?
            .generation;
        let ordered = prepare_cross_project_edges(project, generation, edges)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        replace_cross_project_edges_in(&transaction, project, &ordered)?;
        transaction.commit()?;
        Ok(ordered.len())
    }
}

fn prepare_cross_project_edges(
    project: &ProjectId,
    generation: Generation,
    edges: &[GraphEdge],
) -> Result<Vec<GraphEdge>, StoreError> {
    let mut ordered = edges.to_vec();
    validate_cross_project_edges(project, generation, &mut ordered)?;
    ordered.sort_by(|left, right| {
        (&left.source, &left.target, &left.kind, &left.discriminator).cmp(&(
            &right.source,
            &right.target,
            &right.kind,
            &right.discriminator,
        ))
    });
    ensure_unique_edges(&ordered)?;
    Ok(ordered)
}

fn validate_cross_project_edges(
    project: &ProjectId,
    generation: Generation,
    edges: &mut [GraphEdge],
) -> Result<(), StoreError> {
    for edge in edges {
        if &edge.project != project {
            return Err(StoreError::ProjectMismatch {
                expected: project.clone(),
                actual: edge.project.clone(),
            });
        }
        if !edge.kind.as_str().starts_with("CROSS_") {
            return Err(StoreError::InvalidCrossProjectEdge {
                reason: format!("unsupported kind {}", edge.kind.as_str()),
            });
        }
        edge.generation = generation;
    }
    Ok(())
}

fn ensure_unique_edges(edges: &[GraphEdge]) -> Result<(), StoreError> {
    if edges.windows(2).any(|pair| {
        (
            &pair[0].source,
            &pair[0].target,
            &pair[0].kind,
            &pair[0].discriminator,
        ) == (
            &pair[1].source,
            &pair[1].target,
            &pair[1].kind,
            &pair[1].discriminator,
        )
    }) {
        return Err(StoreError::DuplicateEdge);
    }
    Ok(())
}

fn replace_cross_project_edges_in(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    edges: &[GraphEdge],
) -> Result<(), StoreError> {
    transaction.execute(
        "DELETE FROM edges WHERE project_id = ?1 AND kind LIKE 'CROSS\\_%' ESCAPE '\\'",
        params![project.as_str()],
    )?;
    for edge in edges {
        ensure_node_exists(transaction, project, &edge.source)?;
        ensure_node_exists(transaction, project, &edge.target)?;
        insert_edge(transaction, edge)?;
    }
    Ok(())
}
