use std::collections::BTreeSet;

use goldeneye_domain::{ContentHash, FileId, ProjectId, ProjectRelativePath};
use goldeneye_ports::{EditJournalRecord, EditOperationId, EditRefreshResult, EditRefreshStatus};

use super::{DurableEditError, DurableEditService, FaultPoint};

impl DurableEditService {
    pub(super) fn project(
        &self,
        project_id: &ProjectId,
    ) -> Result<goldeneye_domain::ProjectRecord, DurableEditError> {
        self.journal
            .get_project(project_id)?
            .ok_or_else(|| DurableEditError::ProjectNotFound(project_id.clone()))
    }

    pub(super) fn ensure_indexed_hash(
        &self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
        actual_hash: ContentHash,
    ) -> Result<(), DurableEditError> {
        let file_id = FileId::new(project_id.clone(), path.clone());
        let indexed = self
            .journal
            .get_file(&file_id)?
            .ok_or_else(|| DurableEditError::FileNotIndexed(path.clone()))?;
        if indexed.content_hash != actual_hash {
            return Err(DurableEditError::StaleSource {
                expected: indexed.content_hash,
                actual: actual_hash,
            });
        }
        Ok(())
    }

    pub(super) fn node_ids(
        &self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<BTreeSet<String>, DurableEditError> {
        let file_id = FileId::new(project_id.clone(), path.clone());
        Ok(self
            .journal
            .nodes_for_file(&file_id)?
            .into_iter()
            .map(|node| node.id.as_str().to_owned())
            .collect())
    }

    pub(super) fn refresh_existing(
        &mut self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<EditRefreshResult, DurableEditError> {
        let refresh = self.index.refresh_file(project_id, path)?;
        if refresh.status == EditRefreshStatus::RejectedSyntax {
            return Err(DurableEditError::RefreshRejected {
                reason: format!("parser returned {} diagnostic groups", refresh.diagnostics),
            });
        }
        if !matches!(
            refresh.status,
            EditRefreshStatus::Updated | EditRefreshStatus::Unchanged
        ) {
            return Err(DurableEditError::RefreshRejected {
                reason: format!("unexpected refresh status {:?}", refresh.status),
            });
        }
        Ok(refresh)
    }

    pub(super) fn refresh_absent(
        &mut self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<EditRefreshResult, DurableEditError> {
        let refresh = self.index.refresh_file(project_id, path)?;
        if !matches!(
            refresh.status,
            EditRefreshStatus::Deleted | EditRefreshStatus::Unchanged
        ) {
            return Err(DurableEditError::RefreshRejected {
                reason: format!("unexpected absent-file refresh status {:?}", refresh.status),
            });
        }
        Ok(refresh)
    }

    pub(super) fn operation(
        &self,
        operation_id: &EditOperationId,
    ) -> Result<EditJournalRecord, DurableEditError> {
        self.journal
            .get_edit_operation(operation_id)?
            .ok_or_else(|| DurableEditError::OperationNotFound(operation_id.as_str().to_owned()))
    }

    pub(super) fn check_fault(
        &mut self,
        operation_id: &EditOperationId,
        point: FaultPoint,
    ) -> Result<(), DurableEditError> {
        if let Err(message) = self.fault_injector.check(point) {
            let error = DurableEditError::InjectedFault { point, message };
            self.record_error(operation_id, &error);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn record_error(
        &mut self,
        operation_id: &EditOperationId,
        error: &DurableEditError,
    ) {
        let message = error.to_string();
        let compact = message.chars().take(1024).collect::<String>();
        let _ = self
            .journal
            .set_edit_operation_error(operation_id, Some(&compact));
    }
}
