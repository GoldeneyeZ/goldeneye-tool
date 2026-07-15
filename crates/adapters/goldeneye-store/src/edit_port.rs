use goldeneye_ports::{
    EditJournalRecord as PortJournalRecord, EditOperationId as PortOperationId,
    EditOperationKind as PortOperationKind, EditPhase as PortPhase, EditRepository,
    NewEditJournalRecord as PortNewJournalRecord, PortError,
};

use crate::{
    EditJournalRecord, EditOperationId, EditOperationKind, EditPhase, NewEditJournalRecord, Store,
};

impl EditRepository for Store {
    fn get_project(
        &self,
        project: &goldeneye_domain::ProjectId,
    ) -> Result<Option<goldeneye_domain::ProjectRecord>, PortError> {
        Store::get_project(self, project).map_err(PortError::new)
    }

    fn get_file(
        &self,
        file: &goldeneye_domain::FileId,
    ) -> Result<Option<goldeneye_domain::FileRecord>, PortError> {
        Store::get_file(self, file).map_err(PortError::new)
    }

    fn nodes_for_file(
        &self,
        file: &goldeneye_domain::FileId,
    ) -> Result<Vec<goldeneye_domain::GraphNode>, PortError> {
        Store::nodes_for_file(self, file).map_err(PortError::new)
    }

    fn create_edit_operation(
        &mut self,
        record: &PortNewJournalRecord,
    ) -> Result<PortJournalRecord, PortError> {
        let record = to_store_new_record(record).map_err(PortError::new)?;
        let created = Store::create_edit_operation(self, &record).map_err(PortError::new)?;
        from_store_record(created)
    }

    fn transition_edit_operation(
        &mut self,
        operation_id: &PortOperationId,
        expected: PortPhase,
        next: PortPhase,
    ) -> Result<PortJournalRecord, PortError> {
        let operation_id = to_store_id(operation_id).map_err(PortError::new)?;
        let updated = Store::transition_edit_operation(
            self,
            &operation_id,
            to_store_phase(expected),
            to_store_phase(next),
        )
        .map_err(PortError::new)?;
        from_store_record(updated)
    }

    fn get_edit_operation(
        &self,
        operation_id: &PortOperationId,
    ) -> Result<Option<PortJournalRecord>, PortError> {
        let operation_id = to_store_id(operation_id).map_err(PortError::new)?;
        Store::get_edit_operation(self, &operation_id)
            .map_err(PortError::new)?
            .map(from_store_record)
            .transpose()
    }

    fn list_incomplete_edit_operations(&self) -> Result<Vec<PortJournalRecord>, PortError> {
        Store::list_incomplete_edit_operations(self)
            .map_err(PortError::new)?
            .into_iter()
            .map(from_store_record)
            .collect()
    }

    fn set_edit_operation_error(
        &mut self,
        operation_id: &PortOperationId,
        error: Option<&str>,
    ) -> Result<PortJournalRecord, PortError> {
        let operation_id = to_store_id(operation_id).map_err(PortError::new)?;
        let updated =
            Store::set_edit_operation_error(self, &operation_id, error).map_err(PortError::new)?;
        from_store_record(updated)
    }
}

fn to_store_new_record(
    record: &PortNewJournalRecord,
) -> Result<NewEditJournalRecord, crate::StoreError> {
    Ok(NewEditJournalRecord {
        operation_id: to_store_id(&record.operation_id)?,
        operation_kind: to_store_kind(record.operation_kind),
        project_id: record.project_id.clone(),
        path: record.path.clone(),
        original_hash: record.original_hash,
        new_hash: record.new_hash,
        temp_path: record.temp_path.clone(),
        backup_path: record.backup_path.clone(),
        created_parent_paths: record.created_parent_paths.clone(),
    })
}

fn to_store_id(operation_id: &PortOperationId) -> Result<EditOperationId, crate::StoreError> {
    EditOperationId::new(operation_id.as_str())
}

const fn to_store_kind(kind: PortOperationKind) -> EditOperationKind {
    match kind {
        PortOperationKind::Create => EditOperationKind::Create,
        PortOperationKind::Update => EditOperationKind::Update,
        PortOperationKind::Delete => EditOperationKind::Delete,
    }
}

const fn to_store_phase(phase: PortPhase) -> EditPhase {
    match phase {
        PortPhase::Prepared => EditPhase::Prepared,
        PortPhase::BackupReady => EditPhase::BackupReady,
        PortPhase::Replaced => EditPhase::Replaced,
        PortPhase::Indexed => EditPhase::Indexed,
        PortPhase::Committed => EditPhase::Committed,
        PortPhase::RolledBack => EditPhase::RolledBack,
    }
}

fn from_store_record(record: EditJournalRecord) -> Result<PortJournalRecord, PortError> {
    Ok(PortJournalRecord {
        operation_id: PortOperationId::new(record.operation_id.as_str())?,
        record_version: record.record_version,
        operation_kind: from_store_kind(record.operation_kind),
        project_id: record.project_id,
        path: record.path,
        original_hash: record.original_hash,
        new_hash: record.new_hash,
        temp_path: record.temp_path,
        backup_path: record.backup_path,
        created_parent_paths: record.created_parent_paths,
        phase: from_store_phase(record.phase),
        created_at: record.created_at,
        updated_at: record.updated_at,
        last_error: record.last_error,
    })
}

const fn from_store_kind(kind: EditOperationKind) -> PortOperationKind {
    match kind {
        EditOperationKind::Create => PortOperationKind::Create,
        EditOperationKind::Update => PortOperationKind::Update,
        EditOperationKind::Delete => PortOperationKind::Delete,
    }
}

const fn from_store_phase(phase: EditPhase) -> PortPhase {
    match phase {
        EditPhase::Prepared => PortPhase::Prepared,
        EditPhase::BackupReady => PortPhase::BackupReady,
        EditPhase::Replaced => PortPhase::Replaced,
        EditPhase::Indexed => PortPhase::Indexed,
        EditPhase::Committed => PortPhase::Committed,
        EditPhase::RolledBack => PortPhase::RolledBack,
    }
}
