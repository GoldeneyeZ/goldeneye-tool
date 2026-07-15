use goldeneye_ports::{EditJournalRecord, EditOperationId, EditPhase, EditRefreshResult};

use super::{
    ArtifactPaths, DurableEditError, DurableEditService, FaultPoint, ensure_file_hash,
    hard_link_new, metadata, remove_if_exists, rename_new, required_hash, sync_parent, write_temp,
};
use crate::path_auth::{AuthorizedPath, CreatedDirectories};

impl DurableEditService {
    pub(super) fn commit_update(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        source: &[u8],
    ) -> Result<EditRefreshResult, DurableEditError> {
        let record = self.prepare_update(operation_id, authorized, artifacts, source)?;
        self.backup_update(operation_id, authorized, artifacts)?;
        self.install_update(operation_id, authorized, artifacts)?;
        let refresh =
            self.refresh_update_or_rollback(operation_id, authorized, artifacts, &record)?;
        self.finish_update(operation_id, authorized, artifacts)?;
        Ok(refresh)
    }

    fn prepare_update(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        source: &[u8],
    ) -> Result<EditJournalRecord, DurableEditError> {
        self.check_fault(operation_id, FaultPoint::AfterJournal)?;
        self.check_fault(operation_id, FaultPoint::BeforeWrite)?;
        let permissions = metadata(authorized.destination())?.permissions();
        write_temp(&artifacts.temp_absolute, source, Some(permissions))?;
        self.check_fault(operation_id, FaultPoint::AfterTemp)?;
        let record = self.operation(operation_id)?;
        let expected_old = required_hash(record.original_hash, operation_id, "original")?;
        let expected_new = required_hash(record.new_hash, operation_id, "new")?;
        authorized.revalidate()?;
        ensure_file_hash(authorized.destination(), expected_old)?;
        ensure_file_hash(&artifacts.temp_absolute, expected_new)?;
        Ok(record)
    }

    fn backup_update(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
    ) -> Result<(), DurableEditError> {
        rename_new(authorized.destination(), &artifacts.backup_absolute)?;
        sync_parent(authorized.destination())?;
        self.journal.transition_edit_operation(
            operation_id,
            EditPhase::Prepared,
            EditPhase::BackupReady,
        )?;
        self.check_fault(operation_id, FaultPoint::AfterBackup)
    }

    fn install_update(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
    ) -> Result<(), DurableEditError> {
        rename_new(&artifacts.temp_absolute, authorized.destination())?;
        sync_parent(authorized.destination())?;
        self.journal.transition_edit_operation(
            operation_id,
            EditPhase::BackupReady,
            EditPhase::Replaced,
        )?;
        self.check_fault(operation_id, FaultPoint::AfterRename)?;
        self.check_fault(operation_id, FaultPoint::DuringReindex)
    }

    fn refresh_update_or_rollback(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        record: &EditJournalRecord,
    ) -> Result<EditRefreshResult, DurableEditError> {
        match self.refresh_existing(&record.project_id, &record.path) {
            Ok(refresh) => Ok(refresh),
            Err(refresh_error) => {
                let reason = refresh_error.to_string();
                if let Err(rollback) = self.rollback_update(operation_id, authorized, artifacts) {
                    return Err(DurableEditError::RecoveryRequired {
                        operation_id: operation_id.as_str().to_owned(),
                        reason: format!(
                            "index refresh failed ({reason}); rollback failed ({rollback})"
                        ),
                    });
                }
                Err(DurableEditError::RefreshRejected { reason })
            }
        }
    }

    fn finish_update(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
    ) -> Result<(), DurableEditError> {
        self.journal.transition_edit_operation(
            operation_id,
            EditPhase::Replaced,
            EditPhase::Indexed,
        )?;
        self.check_fault(operation_id, FaultPoint::Cleanup)?;
        remove_if_exists(&artifacts.backup_absolute)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        sync_parent(authorized.destination())?;
        self.journal.transition_edit_operation(
            operation_id,
            EditPhase::Indexed,
            EditPhase::Committed,
        )?;
        Ok(())
    }

    pub(super) fn commit_create(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        source: &[u8],
        create_parents: bool,
    ) -> Result<EditRefreshResult, DurableEditError> {
        let (record, created) = self.prepare_create_commit(
            operation_id,
            authorized,
            artifacts,
            source,
            create_parents,
        )?;
        self.mark_create_backup_ready(operation_id, authorized, artifacts, &record)?;
        self.install_create(operation_id, authorized, artifacts)?;
        let refresh = self.refresh_create_or_rollback(
            operation_id,
            authorized,
            artifacts,
            &record,
            created.as_ref(),
        )?;
        self.finish_create_commit(operation_id, authorized, artifacts)?;
        Ok(refresh)
    }

    fn prepare_create_commit(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        source: &[u8],
        create_parents: bool,
    ) -> Result<(EditJournalRecord, Option<CreatedDirectories>), DurableEditError> {
        self.check_fault(operation_id, FaultPoint::AfterJournal)?;
        self.check_fault(operation_id, FaultPoint::BeforeWrite)?;
        let created = create_parents
            .then(|| authorized.create_parent_directories())
            .transpose()?;
        write_temp(&artifacts.temp_absolute, source, None)?;
        self.check_fault(operation_id, FaultPoint::AfterTemp)?;
        Ok((self.operation(operation_id)?, created))
    }

    fn mark_create_backup_ready(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        record: &EditJournalRecord,
    ) -> Result<(), DurableEditError> {
        let expected_new = required_hash(record.new_hash, operation_id, "new")?;
        ensure_file_hash(&artifacts.temp_absolute, expected_new)?;
        authorized.revalidate()?;
        self.journal.transition_edit_operation(
            operation_id,
            EditPhase::Prepared,
            EditPhase::BackupReady,
        )?;
        self.check_fault(operation_id, FaultPoint::AfterBackup)
    }

    fn install_create(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
    ) -> Result<(), DurableEditError> {
        hard_link_new(&artifacts.temp_absolute, authorized.destination())?;
        remove_if_exists(&artifacts.temp_absolute)?;
        sync_parent(authorized.destination())?;
        self.journal.transition_edit_operation(
            operation_id,
            EditPhase::BackupReady,
            EditPhase::Replaced,
        )?;
        self.check_fault(operation_id, FaultPoint::AfterRename)?;
        self.check_fault(operation_id, FaultPoint::DuringReindex)
    }

    fn refresh_create_or_rollback(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        record: &EditJournalRecord,
        created: Option<&CreatedDirectories>,
    ) -> Result<EditRefreshResult, DurableEditError> {
        match self.refresh_existing(&record.project_id, &record.path) {
            Ok(refresh) => Ok(refresh),
            Err(refresh_error) => {
                let reason = refresh_error.to_string();
                if let Err(rollback) =
                    self.rollback_create(operation_id, authorized, artifacts, created)
                {
                    return Err(DurableEditError::RecoveryRequired {
                        operation_id: operation_id.as_str().to_owned(),
                        reason: format!(
                            "index refresh failed ({reason}); rollback failed ({rollback})"
                        ),
                    });
                }
                Err(DurableEditError::RefreshRejected { reason })
            }
        }
    }

    fn finish_create_commit(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
    ) -> Result<(), DurableEditError> {
        self.journal.transition_edit_operation(
            operation_id,
            EditPhase::Replaced,
            EditPhase::Indexed,
        )?;
        self.check_fault(operation_id, FaultPoint::Cleanup)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        sync_parent(authorized.destination())?;
        self.journal.transition_edit_operation(
            operation_id,
            EditPhase::Indexed,
            EditPhase::Committed,
        )?;
        Ok(())
    }

    fn rollback_update(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
    ) -> Result<(), DurableEditError> {
        let record = self.operation(operation_id)?;
        let expected_old = required_hash(record.original_hash, operation_id, "original")?;
        let expected_new = required_hash(record.new_hash, operation_id, "new")?;
        ensure_file_hash(authorized.destination(), expected_new)?;
        ensure_file_hash(&artifacts.backup_absolute, expected_old)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        rename_new(authorized.destination(), &artifacts.temp_absolute)?;
        rename_new(&artifacts.backup_absolute, authorized.destination())?;
        sync_parent(authorized.destination())?;
        self.refresh_existing(&record.project_id, &record.path)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        self.journal.transition_edit_operation(
            operation_id,
            record.phase,
            EditPhase::RolledBack,
        )?;
        Ok(())
    }

    fn rollback_create(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        created: Option<&CreatedDirectories>,
    ) -> Result<(), DurableEditError> {
        let record = self.operation(operation_id)?;
        let expected_new = required_hash(record.new_hash, operation_id, "new")?;
        ensure_file_hash(authorized.destination(), expected_new)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        rename_new(authorized.destination(), &artifacts.temp_absolute)?;
        sync_parent(authorized.destination())?;
        self.refresh_absent(&record.project_id, &record.path)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        if let Some(created) = created {
            created.rollback_empty()?;
        }
        self.journal.transition_edit_operation(
            operation_id,
            record.phase,
            EditPhase::RolledBack,
        )?;
        Ok(())
    }
}
