use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use goldeneye_edit::{
    DurableCreateRequest, DurableEditRequest, DurableEditService, EditOperation, EditOptions,
    ParsePolicy, RecoveryReport,
};
use goldeneye_index::{IndexOptions, IndexService};
use goldeneye_ports::EditInspectRequest;

use crate::{LanguageId, ServiceError, ServiceErrorCode, Services};

use super::{
    CreateFileRequest, DeleteNodeRequest, EditMutationResult, EditParsePolicy,
    InspectSyntaxRequest, InspectSyntaxResult, NodeContentRequest,
    inspection::{edit_error_with_fresh, inspect_file},
    results::mutation_result,
};

impl Services {
    /// Opens durable edit state and reconciles incomplete journal entries.
    ///
    /// # Errors
    ///
    /// Returns configuration, storage, authorization, or journal failures.
    pub fn open(
        config: crate::ServiceConfig,
        dependencies: crate::ServiceDependencies,
    ) -> Result<(Self, RecoveryReport), ServiceError> {
        if !config.database_path().is_file()
            && dependencies.artifact().exists(config.project_root())
        {
            let _ = dependencies
                .artifact()
                .import(config.project_root(), config.database_path());
        }
        let services = Self::new(config, dependencies);
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
        let syntax = self.dependencies.edit_syntax();
        self.with_edit_service(|service| {
            inspect_file(
                service,
                request,
                self.allowed_roots(),
                self.dependencies.languages(),
                syntax.as_ref(),
            )
        })
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
        let language_id = self.create_language_id(request)?;
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
        let languages = self.dependencies.languages();
        let syntax = self.dependencies.edit_syntax();
        self.with_edit_service(|service| match service.edit_node(durable) {
            Ok(result) => Ok(mutation_result(result)),
            Err(error) => Err(edit_error_with_fresh(
                service,
                &locator,
                error,
                allowed_roots,
                languages,
                syntax.as_ref(),
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

    fn create_language_id(&self, request: &CreateFileRequest) -> Result<LanguageId, ServiceError> {
        request
            .language_id
            .clone()
            .or_else(|| {
                self.dependencies
                    .languages()
                    .classify(Path::new(request.path.as_str()))
            })
            .ok_or_else(|| {
                ServiceError::edit(
                    ServiceErrorCode::InvalidInput,
                    format!(
                        "cannot detect supported language for {}",
                        request.path.as_str()
                    ),
                )
            })
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
        let languages = self.dependencies.languages();
        let syntax = self.dependencies.edit_syntax();
        self.with_edit_service(|service| match service.edit_node(durable) {
            Ok(result) => Ok(mutation_result(result)),
            Err(error) => Err(edit_error_with_fresh(
                service,
                &locator,
                error,
                allowed_roots,
                languages,
                syntax.as_ref(),
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
        let store = self
            .dependencies
            .repositories()
            .open_index(self.config.database_path())
            .map_err(ServiceError::Repository)?;
        let index = IndexService::new(
            store,
            self.dependencies.index_syntax(),
            IndexOptions::default(),
            self.dependencies.discovery(),
        );
        let journal = self
            .dependencies
            .repositories()
            .open_edit(self.config.database_path())
            .map_err(ServiceError::Repository)?;
        DurableEditService::open(
            index,
            journal,
            self.dependencies.edit_syntax(),
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

fn edit_options(parse_policy: EditParsePolicy) -> EditOptions {
    EditOptions {
        parse_policy: ParsePolicy::from(parse_policy),
        refresh_request: EditInspectRequest::default(),
    }
}
