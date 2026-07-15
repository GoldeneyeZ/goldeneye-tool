//! Tool-neutral structural edit requests and compact results.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use goldeneye_discovery::{FileSystemDiscovery, LanguageRegistry};
use goldeneye_domain::{ContentHash, FileContext, FileId, SourceSpan};
use goldeneye_edit::path_auth::{PathAuthorizationError, PathAuthorizer, PathIntent};
use goldeneye_edit::{
    DurableCreateRequest, DurableEditError, DurableEditRequest, DurableEditService, EditError,
    EditOperation, EditOptions, MutationResult as DurableMutationResult, ParsePolicy,
    RecoveryReport,
};
use goldeneye_index::{IndexOptions, IndexService};
use goldeneye_ports::{EditDiagnosticKind, EditInspectRequest, EditSyntaxDiagnostic};
use goldeneye_store::Store;
use goldeneye_syntax::{
    CoreGrammarProvider, DiagnosticKind, InspectRequest, SyntaxDiagnostic, SyntaxEngine,
    SyntaxInspection, inspect_syntax as inspect_tree,
};
use serde::{Deserialize, Serialize};

use crate::{
    Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath, ServiceError,
    ServiceErrorCode, Services,
};

const BYTES_PER_APPROXIMATE_TOKEN: usize = 4;

/// Parse validation policy shared by create and structural edit tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditParsePolicy {
    RequireClean,
    NoAdditionalDiagnostics,
    AllowErrors,
}

impl From<EditParsePolicy> for ParsePolicy {
    fn from(value: EditParsePolicy) -> Self {
        match value {
            EditParsePolicy::RequireClean => Self::RequireClean,
            EditParsePolicy::NoAdditionalDiagnostics => Self::NoAdditionalDiagnostics,
            EditParsePolicy::AllowErrors => Self::AllowErrors,
        }
    }
}

const fn default_edit_parse_policy() -> EditParsePolicy {
    EditParsePolicy::NoAdditionalDiagnostics
}

const fn default_create_parse_policy() -> EditParsePolicy {
    EditParsePolicy::RequireClean
}

/// Compact syntax inspection for one indexed project-relative file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectSyntaxRequest {
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    #[serde(default)]
    pub inspect: InspectRequest,
}

impl InspectSyntaxRequest {
    #[must_use]
    pub fn new(project: ProjectId, path: ProjectRelativePath) -> Self {
        Self {
            project,
            path,
            inspect: InspectRequest::default(),
        }
    }
}

/// Content-bearing request shared by replace and adjacent insert operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeContentRequest {
    pub operation_id: String,
    pub locator: NodeLocator,
    pub content: String,
    #[serde(default = "default_edit_parse_policy")]
    pub parse_policy: EditParsePolicy,
}

impl NodeContentRequest {
    #[must_use]
    pub fn new(
        operation_id: impl Into<String>,
        locator: NodeLocator,
        content: impl Into<String>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            locator,
            content: content.into(),
            parse_policy: default_edit_parse_policy(),
        }
    }
}

/// Exact named-node deletion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteNodeRequest {
    pub operation_id: String,
    pub locator: NodeLocator,
    #[serde(default = "default_edit_parse_policy")]
    pub parse_policy: EditParsePolicy,
}

impl DeleteNodeRequest {
    #[must_use]
    pub fn new(operation_id: impl Into<String>, locator: NodeLocator) -> Self {
        Self {
            operation_id: operation_id.into(),
            locator,
            parse_policy: default_edit_parse_policy(),
        }
    }
}

/// No-overwrite project-relative file creation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFileRequest {
    pub operation_id: String,
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    pub content: String,
    pub expected_generation: Generation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_id: Option<LanguageId>,
    #[serde(default = "default_create_parse_policy")]
    pub parse_policy: EditParsePolicy,
    #[serde(default)]
    pub create_parents: bool,
}

impl CreateFileRequest {
    #[must_use]
    pub fn new(
        operation_id: impl Into<String>,
        project: ProjectId,
        path: ProjectRelativePath,
        content: impl Into<String>,
        expected_generation: Generation,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            project,
            path,
            content: content.into(),
            expected_generation,
            language_id: None,
            parse_policy: default_create_parse_policy(),
            create_parents: false,
        }
    }

    #[must_use]
    pub const fn with_parent_creation(mut self, create_parents: bool) -> Self {
        self.create_parents = create_parents;
        self
    }
}

/// JSON-safe diagnostic returned without Tree-sitter implementation details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntaxDiagnosticResult {
    pub kind: String,
    pub node_kind: String,
    pub span: SourceSpan,
}

/// Bounded parse diagnostic summary before and after a mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationDiagnostics {
    pub before_total: usize,
    pub after_total: usize,
    pub before_truncated: bool,
    pub after_truncated: bool,
    pub after: Vec<SyntaxDiagnosticResult>,
}

/// Minimal byte-span diff. Changed content is represented by size and hashes, not echoed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationDiff {
    pub old_span: goldeneye_domain::ByteSpan,
    pub new_span: goldeneye_domain::ByteSpan,
    pub removed_hash: ContentHash,
    pub inserted_hash: ContentHash,
    pub inserted_bytes: usize,
}

/// Compact graph delta and bounded changed stable IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphMutation {
    pub added: usize,
    pub removed: usize,
    pub retained: usize,
}

/// Token-oriented mutation result sizing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationSize {
    pub source_bytes: usize,
    pub changed_bytes: usize,
    pub compact_syntax_bytes: usize,
    pub refreshed_locator_bytes: usize,
    pub approximate_context_tokens: usize,
}

/// Durable mutation output shared by direct services and MCP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditMutationResult {
    pub operation_id: String,
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    pub old_file_hash: Option<ContentHash>,
    pub new_file_hash: ContentHash,
    pub diff: MutationDiff,
    pub changed_syntax_ids: Vec<NodeLocator>,
    pub changed_graph_ids: Vec<String>,
    pub graph: GraphMutation,
    pub generation: Generation,
    pub diagnostics: MutationDiagnostics,
    pub size: MutationSize,
}

/// Token-oriented inspection size metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectionSize {
    pub source_bytes: usize,
    pub compact_syntax_bytes: usize,
    pub locator_bytes: usize,
    pub approximate_context_tokens: usize,
}

/// Compact syntax plus fully serializable locators in matching node order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectSyntaxResult {
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    pub language_id: LanguageId,
    pub file_hash: ContentHash,
    pub generation: Generation,
    pub syntax: SyntaxInspection,
    pub locators: Vec<NodeLocator>,
    pub diagnostic_total: usize,
    pub diagnostics_truncated: bool,
    pub diagnostics: Vec<SyntaxDiagnosticResult>,
    pub size: InspectionSize,
}

impl Services {
    /// Opens durable edit state and reconciles incomplete journal entries.
    ///
    /// # Errors
    ///
    /// Returns configuration, storage, authorization, or journal failures.
    pub fn open(config: crate::ServiceConfig) -> Result<(Self, RecoveryReport), ServiceError> {
        if !config.database_path().is_file()
            && goldeneye_artifact::artifact_exists(config.project_root())
        {
            let _ =
                goldeneye_artifact::import_artifact(config.project_root(), config.database_path());
        }
        let services = Self::new(config);
        let recovery = services.recover_edits()?;
        Ok((services, recovery))
    }

    /// Reconciles every incomplete edit journal entry.
    ///
    /// # Errors
    ///
    /// Returns a typed service error when durable edit state cannot open or recover.
    pub fn recover_edits(&self) -> Result<RecoveryReport, ServiceError> {
        let mut guard = self.edit.lock().map_err(|_| {
            ServiceError::edit(ServiceErrorCode::Storage, "edit service lock poisoned")
        })?;
        if let Some(service) = guard.as_mut() {
            return service.recover_incomplete().map_err(ServiceError::from);
        }
        let (service, recovery) = self.build_edit_service()?;
        *guard = Some(service);
        Ok(recovery)
    }

    /// Returns compact named syntax and full locators for one indexed file.
    ///
    /// # Errors
    ///
    /// Returns typed not-found, stale-source, authorization, parse, or storage failures.
    pub fn inspect_syntax(
        &self,
        request: &InspectSyntaxRequest,
    ) -> Result<InspectSyntaxResult, ServiceError> {
        self.with_edit_service(|service| inspect_file(service, request, self.allowed_roots()))
    }

    /// Creates one new file without overwriting an existing destination.
    ///
    /// # Errors
    ///
    /// Returns typed stale, conflict, authorization, parse, I/O, journal, or index failures.
    pub fn create_file(
        &self,
        request: &CreateFileRequest,
    ) -> Result<EditMutationResult, ServiceError> {
        let language_id = request.language_id.clone().or_else(|| {
            LanguageRegistry::upstream()
                .classify(Path::new(request.path.as_str()))
                .cloned()
        });
        let Some(language_id) = language_id else {
            return Err(ServiceError::edit(
                ServiceErrorCode::InvalidInput,
                format!(
                    "cannot detect supported language for {}",
                    request.path.as_str()
                ),
            ));
        };
        self.with_edit_service(|service| {
            service
                .create_file(DurableCreateRequest {
                    operation_id: request.operation_id.clone(),
                    project_id: request.project.clone(),
                    relative_path: request.path.clone(),
                    language_id,
                    source: Arc::<[u8]>::from(request.content.as_bytes()),
                    expected_generation: request.expected_generation,
                    parse_policy: request.parse_policy.into(),
                    create_parents: request.create_parents,
                })
                .map(mutation_result)
                .map_err(ServiceError::from)
        })
    }

    /// Replaces exactly one locator-identified named node.
    ///
    /// # Errors
    ///
    /// Returns typed stale, conflict, authorization, parse, I/O, journal, or index failures.
    pub fn replace_node(
        &self,
        request: &NodeContentRequest,
    ) -> Result<EditMutationResult, ServiceError> {
        self.edit_with_content(request, EditOperation::Replace(request.content.clone()))
    }

    /// Deletes exactly one locator-identified named node.
    ///
    /// # Errors
    ///
    /// Returns typed stale, conflict, authorization, parse, I/O, journal, or index failures.
    pub fn delete_node(
        &self,
        request: &DeleteNodeRequest,
    ) -> Result<EditMutationResult, ServiceError> {
        let durable = DurableEditRequest {
            operation_id: request.operation_id.clone(),
            locator: request.locator.clone(),
            operation: EditOperation::Delete,
            options: edit_options(request.parse_policy),
        };
        let locator = request.locator.clone();
        let allowed_roots = self.allowed_roots();
        self.with_edit_service(|service| match service.edit_node(durable) {
            Ok(result) => Ok(mutation_result(result)),
            Err(error) => Err(edit_error_with_fresh(
                service,
                &locator,
                error,
                allowed_roots,
            )),
        })
    }

    /// Inserts content immediately before one locator-identified named node.
    ///
    /// # Errors
    ///
    /// Returns typed stale, conflict, authorization, parse, I/O, journal, or index failures.
    pub fn insert_before_node(
        &self,
        request: &NodeContentRequest,
    ) -> Result<EditMutationResult, ServiceError> {
        self.edit_with_content(
            request,
            EditOperation::InsertBefore(request.content.clone()),
        )
    }

    /// Inserts content immediately after one locator-identified named node.
    ///
    /// # Errors
    ///
    /// Returns typed stale, conflict, authorization, parse, I/O, journal, or index failures.
    pub fn insert_after_node(
        &self,
        request: &NodeContentRequest,
    ) -> Result<EditMutationResult, ServiceError> {
        self.edit_with_content(request, EditOperation::InsertAfter(request.content.clone()))
    }

    pub(crate) fn ensure_recovery_resolved(report: &RecoveryReport) -> Result<(), ServiceError> {
        let conflicts = report
            .entries
            .iter()
            .filter(|entry| !entry.resolved)
            .map(|entry| {
                format!(
                    "{}:{}:{}",
                    entry.operation_id,
                    entry.relative_path.as_str(),
                    entry.error.as_deref().unwrap_or("unresolved")
                )
            })
            .collect::<Vec<_>>();
        if conflicts.is_empty() {
            Ok(())
        } else {
            Err(ServiceError::edit(
                ServiceErrorCode::Conflict,
                format!(
                    "unresolved edit recovery conflicts: {}",
                    conflicts.join("; ")
                ),
            ))
        }
    }

    fn edit_with_content(
        &self,
        request: &NodeContentRequest,
        operation: EditOperation,
    ) -> Result<EditMutationResult, ServiceError> {
        let durable = DurableEditRequest {
            operation_id: request.operation_id.clone(),
            locator: request.locator.clone(),
            operation,
            options: edit_options(request.parse_policy),
        };
        let locator = request.locator.clone();
        let allowed_roots = self.allowed_roots();
        self.with_edit_service(|service| match service.edit_node(durable) {
            Ok(result) => Ok(mutation_result(result)),
            Err(error) => Err(edit_error_with_fresh(
                service,
                &locator,
                error,
                allowed_roots,
            )),
        })
    }

    fn with_edit_service<T>(
        &self,
        action: impl FnOnce(&mut DurableEditService) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        let mut guard = self.edit.lock().map_err(|_| {
            ServiceError::edit(ServiceErrorCode::Storage, "edit service lock poisoned")
        })?;
        if guard.is_none() {
            let (service, recovery) = self.build_edit_service()?;
            Self::ensure_recovery_resolved(&recovery)?;
            *guard = Some(service);
        }
        action(guard.as_mut().expect("edit service initialized"))
    }

    fn build_edit_service(&self) -> Result<(DurableEditService, RecoveryReport), ServiceError> {
        self.prepare_database()?;
        let store = Store::open(self.config.database_path())?;
        let index = IndexService::new(
            store,
            CoreGrammarProvider,
            IndexOptions::default(),
            FileSystemDiscovery,
        );
        let journal = Store::open(self.config.database_path())?;
        DurableEditService::open(
            index,
            journal,
            SyntaxEngine::new(CoreGrammarProvider),
            self.allowed_roots(),
        )
        .map_err(ServiceError::from)
    }

    fn allowed_roots(&self) -> Vec<PathBuf> {
        vec![
            self.config
                .allowed_root()
                .unwrap_or_else(|| self.config.project_root())
                .to_path_buf(),
        ]
    }
}

#[allow(clippy::too_many_lines)]
fn inspect_file(
    service: &mut DurableEditService,
    request: &InspectSyntaxRequest,
    allowed_roots: Vec<PathBuf>,
) -> Result<InspectSyntaxResult, ServiceError> {
    let project = service.indexed_project(&request.project)?.ok_or_else(|| {
        ServiceError::edit(
            ServiceErrorCode::NotFound,
            format!("project is not indexed: {}", request.project.as_str()),
        )
    })?;
    let file_id = FileId::new(request.project.clone(), request.path.clone());
    let indexed = service.indexed_file(&file_id)?.ok_or_else(|| {
        ServiceError::edit(
            ServiceErrorCode::NotFound,
            format!("file is not indexed: {}", request.path.as_str()),
        )
    })?;
    let authorizer = PathAuthorizer::new(allowed_roots).map_err(DurableEditError::from)?;
    let authorized_path = authorizer
        .authorize(
            &project.root_path,
            request.path.as_str(),
            PathIntent::Update,
        )
        .map_err(DurableEditError::from)?;
    let destination = authorized_path
        .revalidate()
        .map_err(DurableEditError::from)?;
    let source = fs::read(destination.as_path()).map_err(|source| DurableEditError::Io {
        operation: "reading syntax source",
        path: destination.as_path().to_path_buf(),
        source,
    })?;
    let actual_hash = ContentHash::of(&source);
    if actual_hash != indexed.content_hash {
        return Err(ServiceError::from(DurableEditError::StaleSource {
            expected: indexed.content_hash,
            actual: actual_hash,
        }));
    }
    let language_id = LanguageRegistry::upstream()
        .classify(Path::new(request.path.as_str()))
        .cloned()
        .ok_or_else(|| {
            ServiceError::edit(
                ServiceErrorCode::InvalidInput,
                format!(
                    "cannot detect supported language for {}",
                    request.path.as_str()
                ),
            )
        })?;
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let snapshot = engine
        .parse(
            language_id.clone(),
            Arc::<[u8]>::from(source),
            project.generation,
        )
        .map_err(|error| ServiceError::edit(ServiceErrorCode::InvalidInput, error.to_string()))?;
    let context = FileContext::new(request.project.clone(), request.path.clone());
    let syntax = inspect_tree(&snapshot, &context, &request.inspect)
        .map_err(|error| ServiceError::edit(ServiceErrorCode::InvalidInput, error.to_string()))?;
    let locators = syntax
        .nodes
        .iter()
        .map(|node| syntax.locator(node.ordinal))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ServiceError::edit(ServiceErrorCode::InvalidInput, error.to_string()))?;
    let compact_syntax_bytes = serde_json::to_vec(&syntax)
        .map_err(|error| ServiceError::edit(ServiceErrorCode::InvalidInput, error.to_string()))?
        .len();
    let locator_bytes = serde_json::to_vec(&locators)
        .map_err(|error| ServiceError::edit(ServiceErrorCode::InvalidInput, error.to_string()))?
        .len();
    let source_bytes = snapshot.source().len();
    let approximate_context_tokens = source_bytes
        .saturating_add(compact_syntax_bytes)
        .saturating_add(locator_bytes)
        .div_ceil(BYTES_PER_APPROXIMATE_TOKEN);
    Ok(InspectSyntaxResult {
        project: request.project.clone(),
        path: request.path.clone(),
        language_id,
        file_hash: snapshot.file_hash(),
        generation: snapshot.generation(),
        syntax,
        locators,
        diagnostic_total: snapshot.diagnostic_total(),
        diagnostics_truncated: snapshot.diagnostics_truncated(),
        diagnostics: snapshot
            .diagnostics()
            .iter()
            .map(diagnostic_result)
            .collect(),
        size: InspectionSize {
            source_bytes,
            compact_syntax_bytes,
            locator_bytes,
            approximate_context_tokens,
        },
    })
}

fn edit_options(parse_policy: EditParsePolicy) -> EditOptions {
    EditOptions {
        parse_policy: parse_policy.into(),
        refresh_request: EditInspectRequest::default(),
    }
}

fn edit_error_with_fresh(
    service: &mut DurableEditService,
    locator: &NodeLocator,
    error: DurableEditError,
    allowed_roots: Vec<PathBuf>,
) -> ServiceError {
    if !matches!(
        &error,
        DurableEditError::StaleGeneration { .. } | DurableEditError::StaleSource { .. }
    ) {
        return ServiceError::from(error);
    }
    let request = InspectSyntaxRequest::new(
        locator.scope.file.project_id.clone(),
        locator.scope.file.relative_path.clone(),
    );
    let fresh = inspect_file(service, &request, allowed_roots)
        .and_then(|result| {
            serde_json::to_string(&result.syntax).map_err(|serialization| {
                ServiceError::edit(ServiceErrorCode::Storage, serialization.to_string())
            })
        })
        .unwrap_or_else(|fresh_error| format!("<unavailable:{fresh_error}>"));
    ServiceError::edit(
        ServiceErrorCode::Conflict,
        format!("{error}; fresh_syntax={fresh}"),
    )
}

fn mutation_result(result: DurableMutationResult) -> EditMutationResult {
    EditMutationResult {
        operation_id: result.operation_id,
        project: result.project_id,
        path: result.relative_path,
        old_file_hash: result.old_file_hash,
        new_file_hash: result.new_file_hash,
        diff: MutationDiff {
            old_span: result.diff.old_span,
            new_span: result.diff.new_span,
            removed_hash: result.diff.removed_hash,
            inserted_hash: result.diff.inserted_hash,
            inserted_bytes: result.diff.inserted.len(),
        },
        changed_syntax_ids: result.syntax_identities,
        changed_graph_ids: result.changed_graph_identities,
        graph: GraphMutation {
            added: result.graph_changes.added,
            removed: result.graph_changes.removed,
            retained: result.graph_changes.retained,
        },
        generation: result.generation,
        diagnostics: MutationDiagnostics {
            before_total: result.diagnostics.before_total,
            after_total: result.diagnostics.after_total,
            before_truncated: result.diagnostics.before_truncated,
            after_truncated: result.diagnostics.after_truncated,
            after: result
                .diagnostics
                .after
                .iter()
                .map(edit_diagnostic_result)
                .collect(),
        },
        size: MutationSize {
            source_bytes: result.token_size.source_bytes,
            changed_bytes: result.token_size.changed_bytes,
            compact_syntax_bytes: result.token_size.compact_syntax_bytes,
            refreshed_locator_bytes: result.token_size.refreshed_locator_bytes,
            approximate_context_tokens: result.token_size.approximate_context_tokens,
        },
    }
}

fn diagnostic_result(diagnostic: &SyntaxDiagnostic) -> SyntaxDiagnosticResult {
    SyntaxDiagnosticResult {
        kind: match diagnostic.kind {
            DiagnosticKind::Error => "error",
            DiagnosticKind::Missing => "missing",
        }
        .to_owned(),
        node_kind: diagnostic.node_kind.clone(),
        span: diagnostic.span,
    }
}

fn edit_diagnostic_result(diagnostic: &EditSyntaxDiagnostic) -> SyntaxDiagnosticResult {
    SyntaxDiagnosticResult {
        kind: match diagnostic.kind {
            EditDiagnosticKind::Error => "error",
            EditDiagnosticKind::Missing => "missing",
        }
        .to_owned(),
        node_kind: diagnostic.node_kind.clone(),
        span: diagnostic.span,
    }
}

impl ServiceError {
    pub(crate) fn edit(code: ServiceErrorCode, message: impl Into<String>) -> Self {
        Self::Edit {
            code,
            message: message.into(),
        }
    }
}

impl From<DurableEditError> for ServiceError {
    fn from(error: DurableEditError) -> Self {
        if let DurableEditError::Edit(EditError::StaleLocator { cause, fresh }) = error {
            let fresh = serde_json::to_string(&fresh)
                .unwrap_or_else(|serialization| format!("<unavailable:{serialization}>"));
            return Self::edit(
                ServiceErrorCode::Conflict,
                format!("node locator is stale: {cause}; fresh_syntax={fresh}"),
            );
        }
        let code = match &error {
            DurableEditError::Path(PathAuthorizationError::DestinationExists { .. })
            | DurableEditError::StaleGeneration { .. }
            | DurableEditError::StaleSource { .. }
            | DurableEditError::TargetBusy(_)
            | DurableEditError::NoContentChange
            | DurableEditError::RecoveryRequired { .. } => ServiceErrorCode::Conflict,
            DurableEditError::Path(
                PathAuthorizationError::ProjectOutsideAllowedRoots { .. }
                | PathAuthorizationError::ProjectRootChanged { .. }
                | PathAuthorizationError::PathEscapesProject { .. }
                | PathAuthorizationError::ReservedMetadata { .. },
            ) => ServiceErrorCode::Forbidden,
            DurableEditError::Path(
                PathAuthorizationError::DestinationMissing { .. }
                | PathAuthorizationError::DestinationNotFile { .. },
            )
            | DurableEditError::ProjectNotFound(_)
            | DurableEditError::FileNotIndexed(_)
            | DurableEditError::OperationNotFound(_) => ServiceErrorCode::NotFound,
            DurableEditError::Repository(_) | DurableEditError::Io { .. } => {
                ServiceErrorCode::Storage
            }
            DurableEditError::RefreshRejected { .. } => ServiceErrorCode::Index,
            DurableEditError::InjectedFault { .. }
            | DurableEditError::JournalPathMismatch(_)
            | DurableEditError::Path(_)
            | DurableEditError::Edit(_)
            | DurableEditError::Identity(_)
            | DurableEditError::GenerationOverflow(_) => ServiceErrorCode::InvalidInput,
        };
        Self::edit(code, error.to_string())
    }
}
