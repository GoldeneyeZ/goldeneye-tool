use std::path::Path;

use goldeneye_ports::{EditJournalRecord, EditPhase};

use super::{
    ArtifactPaths, DurableEditError, DurableEditService, RecoveryAction, RecoveryEntry,
    RecoveryReport, TargetLease, cleanup_artifacts, hash_if_file, join_relative, path_present,
    remove_empty_confined_directory, remove_if_exists, rename_new, required_hash, sync_parent,
    validate_journal_artifacts,
};
use crate::path_auth::PathIntent;

impl DurableEditService {
    /// Reconciles every nonterminal journal row against authoritative on-disk hashes.
    ///
    /// # Errors
    ///
    /// Returns a store error only when the incomplete journal cannot be listed. Per-operation
    /// conflicts remain journaled with recovery material and are returned as unresolved entries.
    pub fn recover_incomplete(&mut self) -> Result<RecoveryReport, DurableEditError> {
        let records = self.journal.list_incomplete_edit_operations()?;
        let mut entries = Vec::with_capacity(records.len());
        for record in records {
            let operation_id = record.operation_id.as_str().to_owned();
            let project_id = record.project_id.clone();
            let relative_path = record.path.clone();
            match self.recover_one(&record) {
                Ok(action) => entries.push(RecoveryEntry {
                    operation_id,
                    project_id,
                    relative_path,
                    resolved: true,
                    action,
                    error: None,
                }),
                Err(error) => {
                    self.record_error(&record.operation_id, &error);
                    entries.push(RecoveryEntry {
                        operation_id,
                        project_id,
                        relative_path,
                        resolved: false,
                        action: RecoveryAction::PreservedConflict,
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        Ok(RecoveryReport { entries })
    }

    fn recover_one(
        &mut self,
        record: &EditJournalRecord,
    ) -> Result<RecoveryAction, DurableEditError> {
        let project = self.project(&record.project_id)?;
        let lexical = join_relative(Path::new(&project.root_path), &record.path);
        let intent = if path_present(&lexical)? {
            PathIntent::Update
        } else {
            PathIntent::Create
        };
        let authorized =
            self.authorizer
                .authorize(&project.root_path, record.path.as_str(), intent)?;
        let _lease = TargetLease::acquire(authorized.destination())?;
        let artifacts = ArtifactPaths::new(&record.operation_id, &authorized)?;
        validate_journal_artifacts(record, &artifacts)?;

        let actual = hash_if_file(authorized.destination())?;
        if actual.is_some() && actual == record.new_hash {
            self.refresh_existing(&record.project_id, &record.path)?;
            let indexed = self.advance_to_indexed(record)?;
            cleanup_artifacts(&artifacts)?;
            sync_parent(authorized.destination())?;
            self.journal.transition_edit_operation(
                &record.operation_id,
                indexed.phase,
                EditPhase::Committed,
            )?;
            return Ok(RecoveryAction::CommittedNewSource);
        }

        if actual.is_some() && actual == record.original_hash {
            self.refresh_existing(&record.project_id, &record.path)?;
            cleanup_artifacts(&artifacts)?;
            Self::rollback_recorded_parents(authorized.project_root(), record)?;
            self.mark_rolled_back(record)?;
            return Ok(RecoveryAction::RestoredOriginalSource);
        }

        if actual.is_none() {
            if record.original_hash.is_none() {
                self.refresh_absent(&record.project_id, &record.path)?;
                cleanup_artifacts(&artifacts)?;
                Self::rollback_recorded_parents(authorized.project_root(), record)?;
                self.mark_rolled_back(record)?;
                return Ok(RecoveryAction::RemovedIncompleteCreate);
            }
            let expected_old =
                required_hash(record.original_hash, &record.operation_id, "original")?;
            if hash_if_file(&artifacts.backup_absolute)? != Some(expected_old) {
                return Err(DurableEditError::RecoveryRequired {
                    operation_id: record.operation_id.as_str().to_owned(),
                    reason: "target is missing and backup does not match original hash".to_owned(),
                });
            }
            rename_new(&artifacts.backup_absolute, authorized.destination())?;
            sync_parent(authorized.destination())?;
            self.refresh_existing(&record.project_id, &record.path)?;
            remove_if_exists(&artifacts.temp_absolute)?;
            self.mark_rolled_back(record)?;
            return Ok(RecoveryAction::RestoredOriginalSource);
        }

        // An external writer won the race. Make the graph reflect that source, but retain the
        // journal and both known versions for a human/agent conflict decision.
        self.refresh_existing(&record.project_id, &record.path)?;
        Err(DurableEditError::RecoveryRequired {
            operation_id: record.operation_id.as_str().to_owned(),
            reason: format!(
                "actual target hash {} matches neither journal version",
                actual.expect("actual hash is present")
            ),
        })
    }

    fn advance_to_indexed(
        &mut self,
        initial: &EditJournalRecord,
    ) -> Result<EditJournalRecord, DurableEditError> {
        let mut current = self.operation(&initial.operation_id)?;
        while current.phase != EditPhase::Indexed {
            let next = match current.phase {
                EditPhase::Prepared => EditPhase::BackupReady,
                EditPhase::BackupReady => EditPhase::Replaced,
                EditPhase::Replaced => EditPhase::Indexed,
                EditPhase::Indexed => break,
                EditPhase::Committed | EditPhase::RolledBack => {
                    return Err(DurableEditError::RecoveryRequired {
                        operation_id: current.operation_id.as_str().to_owned(),
                        reason: format!("unexpected terminal phase {:?}", current.phase),
                    });
                }
            };
            current = self.journal.transition_edit_operation(
                &current.operation_id,
                current.phase,
                next,
            )?;
        }
        Ok(current)
    }

    fn mark_rolled_back(&mut self, initial: &EditJournalRecord) -> Result<(), DurableEditError> {
        let current = self.operation(&initial.operation_id)?;
        if current.phase != EditPhase::RolledBack {
            self.journal.transition_edit_operation(
                &current.operation_id,
                current.phase,
                EditPhase::RolledBack,
            )?;
        }
        Ok(())
    }

    fn rollback_recorded_parents(
        project_root: &Path,
        record: &EditJournalRecord,
    ) -> Result<(), DurableEditError> {
        for relative in record.created_parent_paths.iter().rev() {
            let path = join_relative(project_root, relative);
            remove_empty_confined_directory(project_root, &path)?;
        }
        Ok(())
    }
}
