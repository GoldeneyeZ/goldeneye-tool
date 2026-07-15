use goldeneye_domain::{Generation, ProjectId};
use goldeneye_ports::{
    NodeSignatureRecord, NodeVectorRecord, PortError, SemanticIndexRepository, TokenVectorRecord,
};

use crate::Store;

impl SemanticIndexRepository for Store {
    fn replace_semantic_index(
        &mut self,
        project: &ProjectId,
        expected_generation: Generation,
        node_vectors: &[NodeVectorRecord],
        token_vectors: &[TokenVectorRecord],
        signatures: &[NodeSignatureRecord],
    ) -> Result<(), PortError> {
        Store::replace_semantic_index(
            self,
            project,
            expected_generation,
            node_vectors,
            token_vectors,
            signatures,
        )
        .map(drop)
        .map_err(PortError::new)
    }
}
