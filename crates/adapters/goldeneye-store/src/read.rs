use super::{
    AdrRecord, ConnectionSettings, EditJournalRecord, EditOperationId, FileId, FileRecord,
    GitCoChangeRecord, GitFileHistoryRecord, GraphCounts, GraphEdge, GraphNode, NodeId,
    NodeSignatureRecord, NodeVectorRecord, ProjectId, ProjectRecord, ProjectRelativePath,
    QualifiedName, QueryStore, RuntimeTraceRecord, SchemaInfo, SearchHit, Store, StoreError,
    TokenVectorRecord, connection_settings, count_search_nodes, counts, coupled_files, edges_from,
    edges_to, get_adr, get_edit_operation, get_file, get_node, get_node_signature, get_node_vector,
    get_project, get_token_vector, list_edges, list_files, list_git_cochanges,
    list_git_file_history, list_incomplete_edit_operations, list_node_signatures,
    list_node_vectors, list_nodes, list_projects, list_runtime_traces, node_by_qualified_name,
    nodes_for_file, schema, search_nodes_page,
};

macro_rules! impl_read_api {
    ($type:ty) => {
        impl $type {
            /// Returns versioned schema metadata.
            ///
            /// # Errors
            ///
            /// Returns a store error when schema introspection fails.
            pub fn schema_info(&self) -> Result<SchemaInfo, StoreError> {
                schema::inspect(&self.connection)
            }

            /// Returns effective connection pragmas.
            ///
            /// # Errors
            ///
            /// Returns a store error when a pragma cannot be read.
            pub fn connection_settings(&self) -> Result<ConnectionSettings, StoreError> {
                connection_settings(&self.connection)
            }

            /// Finds a project registry record by exact case-sensitive ID.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_project(
                &self,
                project: &ProjectId,
            ) -> Result<Option<ProjectRecord>, StoreError> {
                get_project(&self.connection, project)
            }

            /// Lists projects in deterministic ID order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_projects(&self) -> Result<Vec<ProjectRecord>, StoreError> {
                list_projects(&self.connection)
            }

            /// Finds the project ADR when one exists.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_adr(&self, project: &ProjectId) -> Result<Option<AdrRecord>, StoreError> {
                get_adr(&self.connection, project)
            }

            /// Lists aggregated runtime traces in stable caller/callee order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_runtime_traces(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<RuntimeTraceRecord>, StoreError> {
                list_runtime_traces(&self.connection, project)
            }

            /// Lists temporal Git metadata in deterministic path order.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn list_git_file_history(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<GitFileHistoryRecord>, StoreError> {
                list_git_file_history(&self.connection, project)
            }

            /// Lists co-change relationships in deterministic pair order.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn list_git_cochanges(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<GitCoChangeRecord>, StoreError> {
                list_git_cochanges(&self.connection, project)
            }

            /// Returns files historically coupled to one path, strongest first.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn coupled_files(
                &self,
                project: &ProjectId,
                path: &ProjectRelativePath,
            ) -> Result<Vec<GitCoChangeRecord>, StoreError> {
                coupled_files(&self.connection, project, path)
            }

            /// Finds a normalized file record by compound identity.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_file(&self, file: &FileId) -> Result<Option<FileRecord>, StoreError> {
                get_file(&self.connection, file)
            }

            /// Lists a project's files in deterministic path order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_files(&self, project: &ProjectId) -> Result<Vec<FileRecord>, StoreError> {
                list_files(&self.connection, project)
            }

            /// Lists all project nodes by qualified name and stable ID.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, StoreError> {
                list_nodes(&self.connection, project)
            }

            /// Lists all project edges in deterministic identity order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_edges(&self, project: &ProjectId) -> Result<Vec<GraphEdge>, StoreError> {
                list_edges(&self.connection, project)
            }

            /// Lists a file's nodes in deterministic node-ID order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, StoreError> {
                nodes_for_file(&self.connection, file)
            }

            /// Finds a graph node by stable ID.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_node(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Option<GraphNode>, StoreError> {
                get_node(&self.connection, project, node)
            }

            /// Finds a graph node by exact qualified name.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn node_by_qualified_name(
                &self,
                project: &ProjectId,
                qualified_name: &QualifiedName,
            ) -> Result<Option<GraphNode>, StoreError> {
                node_by_qualified_name(&self.connection, project, qualified_name)
            }

            /// Lists outbound edges in deterministic identity order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn edges_from(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Vec<GraphEdge>, StoreError> {
                edges_from(&self.connection, project, node)
            }

            /// Lists inbound edges in deterministic identity order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn edges_to(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Vec<GraphEdge>, StoreError> {
                edges_to(&self.connection, project, node)
            }

            /// Runs a project-scoped `FTS5` query ordered by rank and node ID.
            ///
            /// # Errors
            ///
            /// Returns a store error for invalid `FTS5` syntax or read/decode failure.
            pub fn search_nodes(
                &self,
                project: &ProjectId,
                query: &str,
                limit: usize,
            ) -> Result<Vec<SearchHit>, StoreError> {
                search_nodes_page(&self.connection, project, query, limit, 0)
            }

            /// Runs a deterministic project-scoped `FTS5` page.
            ///
            /// # Errors
            ///
            /// Returns a store error for invalid `FTS5` syntax, numeric overflow, or decode failure.
            pub fn search_nodes_page(
                &self,
                project: &ProjectId,
                query: &str,
                limit: usize,
                offset: usize,
            ) -> Result<Vec<SearchHit>, StoreError> {
                search_nodes_page(&self.connection, project, query, limit, offset)
            }

            /// Counts project-scoped `FTS5` matches without materializing nodes.
            ///
            /// # Errors
            ///
            /// Returns a store error for invalid `FTS5` syntax or read failure.
            pub fn count_search_nodes(
                &self,
                project: &ProjectId,
                query: &str,
            ) -> Result<u64, StoreError> {
                count_search_nodes(&self.connection, project, query)
            }

            /// Lists persisted node vectors in stable node-ID order.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn list_node_vectors(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<NodeVectorRecord>, StoreError> {
                list_node_vectors(&self.connection, project)
            }

            /// Finds one persisted node vector by stable node ID.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn get_node_vector(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Option<NodeVectorRecord>, StoreError> {
                get_node_vector(&self.connection, project, node)
            }

            /// Finds one enriched token vector by exact case-sensitive token.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn get_token_vector(
                &self,
                project: &ProjectId,
                token: &str,
            ) -> Result<Option<TokenVectorRecord>, StoreError> {
                get_token_vector(&self.connection, project, token)
            }

            /// Lists structural signatures in stable node-ID order.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn list_node_signatures(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<NodeSignatureRecord>, StoreError> {
                list_node_signatures(&self.connection, project)
            }

            /// Finds one structural signature by stable node ID.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn get_node_signature(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Option<NodeSignatureRecord>, StoreError> {
                get_node_signature(&self.connection, project, node)
            }

            /// Counts normalized graph records for a project.
            ///
            /// # Errors
            ///
            /// Returns a store error when any count query fails.
            pub fn counts(&self, project: &ProjectId) -> Result<GraphCounts, StoreError> {
                counts(&self.connection, project)
            }

            /// Finds one durable edit journal record by operation ID.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_edit_operation(
                &self,
                operation_id: &EditOperationId,
            ) -> Result<Option<EditJournalRecord>, StoreError> {
                get_edit_operation(&self.connection, operation_id)
            }

            /// Lists recoverable edit operations in deterministic creation order.
            ///
            /// Committed and rolled-back records are terminal and therefore excluded.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_incomplete_edit_operations(
                &self,
            ) -> Result<Vec<EditJournalRecord>, StoreError> {
                list_incomplete_edit_operations(&self.connection)
            }
        }
    };
}

impl_read_api!(Store);
impl_read_api!(QueryStore);
