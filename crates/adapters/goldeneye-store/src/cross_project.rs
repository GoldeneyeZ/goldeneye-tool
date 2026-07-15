use super::{
    GraphEdge, ProjectId, Store, StoreError, TransactionBehavior, ensure_node_exists, insert_edge,
    params,
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
        let mut ordered = edges.to_vec();
        for edge in &mut ordered {
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
        ordered.sort_by(|left, right| {
            (&left.source, &left.target, &left.kind, &left.discriminator).cmp(&(
                &right.source,
                &right.target,
                &right.kind,
                &right.discriminator,
            ))
        });
        if ordered.windows(2).any(|pair| {
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

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM edges WHERE project_id = ?1 AND kind LIKE 'CROSS\\_%' ESCAPE '\\'",
            params![project.as_str()],
        )?;
        for edge in &ordered {
            ensure_node_exists(&transaction, project, &edge.source)?;
            ensure_node_exists(&transaction, project, &edge.target)?;
            insert_edge(&transaction, edge)?;
        }
        transaction.commit()?;
        Ok(ordered.len())
    }
}
