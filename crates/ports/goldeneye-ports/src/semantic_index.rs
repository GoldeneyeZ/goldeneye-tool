use goldeneye_domain::{Generation, ProjectId};

use crate::{NodeSignatureRecord, NodeVectorRecord, PortError, TokenVectorRecord};

/// Semantic-index persistence required by indexing use cases.
pub trait SemanticIndexRepository: Send {
    /// Atomically replaces every semantic artifact for one project generation.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails, or when the durable project
    /// generation no longer matches `expected_generation`.
    fn replace_semantic_index(
        &mut self,
        project: &ProjectId,
        expected_generation: Generation,
        node_vectors: &[NodeVectorRecord],
        token_vectors: &[TokenVectorRecord],
        signatures: &[NodeSignatureRecord],
    ) -> Result<(), PortError>;
}

impl<T> SemanticIndexRepository for Box<T>
where
    T: SemanticIndexRepository + ?Sized,
{
    fn replace_semantic_index(
        &mut self,
        project: &ProjectId,
        expected_generation: Generation,
        node_vectors: &[NodeVectorRecord],
        token_vectors: &[TokenVectorRecord],
        signatures: &[NodeSignatureRecord],
    ) -> Result<(), PortError> {
        self.as_mut().replace_semantic_index(
            project,
            expected_generation,
            node_vectors,
            token_vectors,
            signatures,
        )
    }
}
