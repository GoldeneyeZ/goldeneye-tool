use std::collections::BTreeSet;
use std::sync::Arc;

use goldeneye_domain::{ByteSpan, ContentHash, FileContext, ProjectId, ProjectRelativePath};
use goldeneye_ports::{
    EditOperationId, EditOperationKind, EditRefreshResult, NewEditJournalRecord,
};

use super::{
    ArtifactPaths, DurableCreateRequest, DurableEditError, DurableEditRequest, DurableEditService,
    MutationResult, TargetLease, changed_graph_identities, ensure_generation, graph_changes,
    next_generation, planned_missing_parents, read_file,
};
use crate::path_auth::{AuthorizedPath, PathIntent};
use crate::{
    EditPlan, EditPlanRequest, SourceDiff, ValidatedCreate, plan_edit, validate_create_content,
};

struct PreparedEdit {
    operation_id: String,
    project_id: ProjectId,
    relative_path: ProjectRelativePath,
    authorized: AuthorizedPath,
    plan: EditPlan,
    _lease: TargetLease,
}

struct PreparedCreate {
    operation_id: String,
    project_id: ProjectId,
    relative_path: ProjectRelativePath,
    authorized: AuthorizedPath,
    validated: ValidatedCreate,
    create_parents: bool,
    _lease: TargetLease,
}

impl DurableEditService {
    /// Applies one exact structural edit through the durable journal.
    ///
    /// # Errors
    ///
    /// Returns typed stale, authorization, syntax, I/O, journal, index, or recovery failures.
    pub fn edit_node(
        &mut self,
        request: DurableEditRequest,
    ) -> Result<MutationResult, DurableEditError> {
        let prepared = self.prepare_edit(request)?;
        let before_nodes = self.node_ids(&prepared.project_id, &prepared.relative_path)?;
        let (operation_id, artifacts) = self.begin_update(&prepared)?;
        let refresh = self.commit_update_recording_error(&operation_id, &prepared, &artifacts)?;
        let after_nodes = self.node_ids(&prepared.project_id, &prepared.relative_path)?;
        Ok(finish_edit(prepared, &refresh, &before_nodes, &after_nodes))
    }

    fn prepare_edit(&self, request: DurableEditRequest) -> Result<PreparedEdit, DurableEditError> {
        let project_id = request.locator.scope.file.project_id.clone();
        let relative_path = request.locator.scope.file.relative_path.clone();
        let project = self.project(&project_id)?;
        ensure_generation(request.locator.scope.generation, project.generation)?;
        let authorized = self.authorizer.authorize(
            &project.root_path,
            relative_path.as_str(),
            PathIntent::Update,
        )?;
        let lease = TargetLease::acquire(authorized.destination())?;
        let source = Arc::<[u8]>::from(read_file(authorized.revalidate()?.as_path())?);
        let actual_hash = ensure_request_hash(&request, &source)?;
        self.ensure_indexed_hash(&project_id, &relative_path, actual_hash)?;
        let plan = self.plan_update(
            &request,
            &project_id,
            &relative_path,
            project.generation,
            source,
        )?;
        if plan.old_file_hash == plan.new_file_hash {
            return Err(DurableEditError::NoContentChange);
        }
        Ok(PreparedEdit {
            operation_id: request.operation_id,
            project_id,
            relative_path,
            authorized,
            plan,
            _lease: lease,
        })
    }

    fn plan_update(
        &self,
        request: &DurableEditRequest,
        project_id: &ProjectId,
        relative_path: &ProjectRelativePath,
        generation: goldeneye_domain::Generation,
        source: Arc<[u8]>,
    ) -> Result<EditPlan, DurableEditError> {
        let next_generation = next_generation(project_id, generation)?;
        let file_context = FileContext::new(project_id.clone(), relative_path.clone());
        Ok(plan_edit(
            self.syntax.as_ref(),
            &EditPlanRequest {
                language_id: request.locator.scope.language_id.clone(),
                source,
                current_generation: generation,
                file_context,
                locator: request.locator.clone(),
                operation: request.operation.clone(),
                next_generation,
                options: request.options.clone(),
            },
        )?)
    }

    fn begin_update(
        &mut self,
        prepared: &PreparedEdit,
    ) -> Result<(EditOperationId, ArtifactPaths), DurableEditError> {
        let operation_id = EditOperationId::new(prepared.operation_id.clone())?;
        let artifacts = ArtifactPaths::new(&operation_id, &prepared.authorized)?;
        self.journal.create_edit_operation(&NewEditJournalRecord {
            operation_id: operation_id.clone(),
            operation_kind: EditOperationKind::Update,
            project_id: prepared.project_id.clone(),
            path: prepared.relative_path.clone(),
            original_hash: Some(prepared.plan.old_file_hash),
            new_hash: Some(prepared.plan.new_file_hash),
            temp_path: Some(artifacts.temp_relative.clone()),
            backup_path: Some(artifacts.backup_relative.clone()),
            created_parent_paths: Vec::new(),
        })?;
        Ok((operation_id, artifacts))
    }

    fn commit_update_recording_error(
        &mut self,
        operation_id: &EditOperationId,
        prepared: &PreparedEdit,
        artifacts: &ArtifactPaths,
    ) -> Result<EditRefreshResult, DurableEditError> {
        let outcome = self.commit_update(
            operation_id,
            &prepared.authorized,
            artifacts,
            &prepared.plan.source,
        );
        if let Err(error) = &outcome {
            self.record_error(operation_id, error);
        }
        outcome
    }

    /// Creates one authorized project-relative file without overwriting an existing destination.
    ///
    /// # Errors
    ///
    /// Returns typed stale, authorization, parse, I/O, journal, index, or recovery failures.
    pub fn create_file(
        &mut self,
        request: DurableCreateRequest,
    ) -> Result<MutationResult, DurableEditError> {
        let (prepared, created_parent_paths) = self.prepare_create(request)?;
        let (operation_id, artifacts) = self.begin_create(&prepared, created_parent_paths)?;
        let refresh = self.commit_create_recording_error(&operation_id, &prepared, &artifacts)?;
        let after_nodes = self.node_ids(&prepared.project_id, &prepared.relative_path)?;
        finish_create(prepared, &refresh, &after_nodes)
    }

    fn prepare_create(
        &self,
        request: DurableCreateRequest,
    ) -> Result<(PreparedCreate, Vec<ProjectRelativePath>), DurableEditError> {
        let project = self.project(&request.project_id)?;
        ensure_generation(request.expected_generation, project.generation)?;
        let authorized = self.authorizer.authorize(
            &project.root_path,
            request.relative_path.as_str(),
            PathIntent::Create,
        )?;
        let lease = TargetLease::acquire(authorized.destination())?;
        let next_generation = next_generation(&request.project_id, project.generation)?;
        let file_context =
            FileContext::new(request.project_id.clone(), request.relative_path.clone());
        let validated = validate_create_content(
            self.syntax.as_ref(),
            request.language_id,
            Arc::clone(&request.source),
            next_generation,
            &file_context,
            request.parse_policy,
        )?;
        let created_parent_paths = prepare_parent_paths(&authorized, request.create_parents)?;
        Ok((
            PreparedCreate {
                operation_id: request.operation_id,
                project_id: request.project_id,
                relative_path: request.relative_path,
                authorized,
                validated,
                create_parents: request.create_parents,
                _lease: lease,
            },
            created_parent_paths,
        ))
    }

    fn begin_create(
        &mut self,
        prepared: &PreparedCreate,
        created_parent_paths: Vec<ProjectRelativePath>,
    ) -> Result<(EditOperationId, ArtifactPaths), DurableEditError> {
        let operation_id = EditOperationId::new(prepared.operation_id.clone())?;
        let artifacts = ArtifactPaths::new(&operation_id, &prepared.authorized)?;
        self.journal.create_edit_operation(&NewEditJournalRecord {
            operation_id: operation_id.clone(),
            operation_kind: EditOperationKind::Create,
            project_id: prepared.project_id.clone(),
            path: prepared.relative_path.clone(),
            original_hash: None,
            new_hash: Some(prepared.validated.content_hash),
            temp_path: Some(artifacts.temp_relative.clone()),
            backup_path: None,
            created_parent_paths,
        })?;
        Ok((operation_id, artifacts))
    }

    fn commit_create_recording_error(
        &mut self,
        operation_id: &EditOperationId,
        prepared: &PreparedCreate,
        artifacts: &ArtifactPaths,
    ) -> Result<EditRefreshResult, DurableEditError> {
        let outcome = self.commit_create(
            operation_id,
            &prepared.authorized,
            artifacts,
            &prepared.validated.source,
            prepared.create_parents,
        );
        if let Err(error) = &outcome {
            self.record_error(operation_id, error);
        }
        outcome
    }
}

fn ensure_request_hash(
    request: &DurableEditRequest,
    source: &[u8],
) -> Result<ContentHash, DurableEditError> {
    let actual_hash = ContentHash::of(source);
    if actual_hash != request.locator.scope.file_hash {
        return Err(DurableEditError::StaleSource {
            expected: request.locator.scope.file_hash,
            actual: actual_hash,
        });
    }
    Ok(actual_hash)
}

fn prepare_parent_paths(
    authorized: &AuthorizedPath,
    create_parents: bool,
) -> Result<Vec<ProjectRelativePath>, DurableEditError> {
    if create_parents {
        planned_missing_parents(authorized)
    } else {
        authorized.revalidate()?;
        Ok(Vec::new())
    }
}

fn finish_edit(
    prepared: PreparedEdit,
    refresh: &EditRefreshResult,
    before_nodes: &BTreeSet<String>,
    after_nodes: &BTreeSet<String>,
) -> MutationResult {
    let changed_graph_identities = changed_graph_identities(before_nodes, after_nodes);
    MutationResult {
        operation_id: prepared.operation_id,
        project_id: prepared.project_id,
        relative_path: prepared.relative_path,
        old_file_hash: Some(prepared.plan.old_file_hash),
        new_file_hash: prepared.plan.new_file_hash,
        diff: prepared.plan.diff,
        syntax_identities: prepared.plan.refreshed_locators,
        changed_graph_identities,
        graph_changes: graph_changes(before_nodes, after_nodes),
        generation: refresh.generation,
        diagnostics: prepared.plan.diagnostics,
        token_size: prepared.plan.token_size,
    }
}

fn finish_create(
    prepared: PreparedCreate,
    refresh: &EditRefreshResult,
    after_nodes: &BTreeSet<String>,
) -> Result<MutationResult, DurableEditError> {
    let empty = BTreeSet::new();
    let changed_graph_identities = changed_graph_identities(&empty, after_nodes);
    let mut syntax_identities = prepared.validated.locators;
    syntax_identities.truncate(64);
    let source_len = u64::try_from(prepared.validated.source.len())
        .map_err(|_| DurableEditError::GenerationOverflow(prepared.project_id.clone()))?;
    let diff = create_diff(
        prepared.validated.content_hash,
        &prepared.validated.source,
        source_len,
    )?;
    Ok(MutationResult {
        operation_id: prepared.operation_id,
        project_id: prepared.project_id,
        relative_path: prepared.relative_path,
        old_file_hash: None,
        new_file_hash: prepared.validated.content_hash,
        diff,
        syntax_identities,
        changed_graph_identities,
        graph_changes: graph_changes(&empty, after_nodes),
        generation: refresh.generation,
        diagnostics: prepared.validated.diagnostics,
        token_size: prepared.validated.token_size,
    })
}

fn create_diff(
    content_hash: ContentHash,
    source: &Arc<[u8]>,
    source_len: u64,
) -> Result<SourceDiff, DurableEditError> {
    Ok(SourceDiff {
        old_span: ByteSpan::new(0, 0)?,
        new_span: ByteSpan::new(0, source_len)?,
        removed_hash: ContentHash::of([]),
        inserted_hash: content_hash,
        inserted: Arc::clone(source),
    })
}
