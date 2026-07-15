use std::sync::Arc;

use goldeneye_domain::{
    FileRecord, GraphEdge, GraphNode, LanguageId, NodeId, ProjectRelativePath, SourceSpan,
};

use crate::{IndexMode, PortError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexDiagnosticKind {
    Error,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSyntaxDiagnostic {
    pub kind: IndexDiagnosticKind,
    pub node_kind: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexFileSyntaxDiagnostics {
    pub path: ProjectRelativePath,
    pub total: usize,
    pub truncated: bool,
    pub details: Vec<IndexSyntaxDiagnostic>,
}

#[derive(Clone)]
pub struct IndexExtractionRequest {
    pub record: FileRecord,
    pub language: LanguageId,
    pub source: Arc<[u8]>,
}

pub struct IndexExtractedFile {
    pub record: FileRecord,
    pub source: Arc<[u8]>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub calls: Vec<IndexExtractedCall>,
    pub relations: Vec<IndexExtractedRelation>,
    pub imports: Vec<IndexExtractedImport>,
    pub diagnostics: Option<IndexFileSyntaxDiagnostics>,
}

#[derive(Debug, Clone)]
pub struct IndexExtractedCall {
    pub source: NodeId,
    pub file: ProjectRelativePath,
    pub language: LanguageId,
    pub caller_qn: String,
    pub callee_name: String,
    pub short_name: String,
    pub receiver_type: Option<String>,
    pub start_byte: u64,
    pub line: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndexExtractedRelation {
    pub source: NodeId,
    pub file: ProjectRelativePath,
    pub language: LanguageId,
    pub kind: &'static str,
    pub target_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndexExtractedImport {
    pub file: ProjectRelativePath,
    pub language: LanguageId,
    pub alias: String,
    pub module_path: String,
}

/// Syntax-backed source extraction required by repository indexing.
pub trait IndexSyntaxExtractor: Send + Sync {
    /// Returns every language identifier this extractor can parse.
    fn supported_ids(&self) -> Vec<LanguageId>;

    /// Parses one source file and emits graph-local facts without project-wide resolution.
    ///
    /// # Errors
    ///
    /// Returns an adapter failure when parsing, coordinate conversion, or graph fact creation
    /// fails.
    fn extract(
        &self,
        request: IndexExtractionRequest,
        mode: IndexMode,
    ) -> Result<IndexExtractedFile, PortError>;
}

impl<T> IndexSyntaxExtractor for Arc<T>
where
    T: IndexSyntaxExtractor + ?Sized,
{
    fn supported_ids(&self) -> Vec<LanguageId> {
        self.as_ref().supported_ids()
    }

    fn extract(
        &self,
        request: IndexExtractionRequest,
        mode: IndexMode,
    ) -> Result<IndexExtractedFile, PortError> {
        self.as_ref().extract(request, mode)
    }
}
