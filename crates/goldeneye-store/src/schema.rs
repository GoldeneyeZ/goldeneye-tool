use std::collections::BTreeSet;

use rusqlite::{Connection, TransactionBehavior, params};

use crate::{SchemaInfo, StoreError};

pub const CURRENT_SCHEMA_VERSION: u32 = 5;

const MIGRATIONS: &[(u32, &str)] = &[
    (
        1,
        r"
        CREATE TABLE projects (
            id TEXT PRIMARY KEY COLLATE BINARY,
            root_path TEXT NOT NULL,
            current_generation INTEGER NOT NULL DEFAULT 0 CHECK (current_generation >= 0)
        ) STRICT;

        CREATE TABLE files (
            project_id TEXT NOT NULL COLLATE BINARY,
            path TEXT NOT NULL COLLATE BINARY,
            content_hash TEXT NOT NULL CHECK (length(content_hash) = 64),
            generation INTEGER NOT NULL CHECK (generation >= 0),
            modified_ns INTEGER NOT NULL CHECK (modified_ns >= 0),
            byte_len INTEGER NOT NULL CHECK (byte_len >= 0),
            PRIMARY KEY (project_id, path),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;

        CREATE TABLE nodes (
            row_id INTEGER PRIMARY KEY,
            project_id TEXT NOT NULL COLLATE BINARY,
            node_id TEXT NOT NULL COLLATE BINARY,
            label TEXT NOT NULL,
            name TEXT NOT NULL,
            qualified_name TEXT NOT NULL COLLATE BINARY,
            file_path TEXT COLLATE BINARY,
            start_byte INTEGER,
            end_byte INTEGER,
            start_row INTEGER,
            start_column INTEGER,
            end_row INTEGER,
            end_column INTEGER,
            generation INTEGER NOT NULL CHECK (generation >= 0),
            properties_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(properties_json)),
            UNIQUE (project_id, node_id),
            UNIQUE (project_id, qualified_name),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY (project_id, file_path) REFERENCES files(project_id, path) ON DELETE CASCADE,
            CHECK (
                (start_byte IS NULL AND end_byte IS NULL AND start_row IS NULL
                 AND start_column IS NULL AND end_row IS NULL AND end_column IS NULL)
                OR
                (start_byte IS NOT NULL AND end_byte IS NOT NULL AND start_row IS NOT NULL
                 AND start_column IS NOT NULL AND end_row IS NOT NULL AND end_column IS NOT NULL)
            )
        ) STRICT;

        CREATE TABLE edges (
            row_id INTEGER PRIMARY KEY,
            project_id TEXT NOT NULL COLLATE BINARY,
            source_id TEXT NOT NULL COLLATE BINARY,
            target_id TEXT NOT NULL COLLATE BINARY,
            kind TEXT NOT NULL COLLATE BINARY,
            discriminator TEXT NOT NULL DEFAULT '' COLLATE BINARY,
            generation INTEGER NOT NULL CHECK (generation >= 0),
            properties_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(properties_json)),
            UNIQUE (project_id, source_id, target_id, kind, discriminator),
            FOREIGN KEY (project_id, source_id) REFERENCES nodes(project_id, node_id) ON DELETE CASCADE,
            FOREIGN KEY (project_id, target_id) REFERENCES nodes(project_id, node_id) ON DELETE CASCADE
        ) STRICT;

        CREATE INDEX files_generation_idx ON files(project_id, generation, path);
        CREATE INDEX nodes_file_idx ON nodes(project_id, file_path, node_id);
        CREATE INDEX nodes_label_idx ON nodes(project_id, label, node_id);
        CREATE INDEX edges_source_idx ON edges(project_id, source_id, kind, target_id);
        CREATE INDEX edges_target_idx ON edges(project_id, target_id, kind, source_id);
        ",
    ),
    (
        2,
        r"
        CREATE VIRTUAL TABLE nodes_fts USING fts5(
            name,
            qualified_name,
            label,
            file_path,
            content='nodes',
            content_rowid='row_id',
            tokenize='unicode61 remove_diacritics 2'
        );

        CREATE TRIGGER nodes_fts_insert AFTER INSERT ON nodes BEGIN
            INSERT INTO nodes_fts(rowid, name, qualified_name, label, file_path)
            VALUES (new.row_id, new.name, new.qualified_name, new.label, coalesce(new.file_path, ''));
        END;

        CREATE TRIGGER nodes_fts_delete AFTER DELETE ON nodes BEGIN
            INSERT INTO nodes_fts(nodes_fts, rowid, name, qualified_name, label, file_path)
            VALUES ('delete', old.row_id, old.name, old.qualified_name, old.label, coalesce(old.file_path, ''));
        END;

        CREATE TRIGGER nodes_fts_update AFTER UPDATE ON nodes BEGIN
            INSERT INTO nodes_fts(nodes_fts, rowid, name, qualified_name, label, file_path)
            VALUES ('delete', old.row_id, old.name, old.qualified_name, old.label, coalesce(old.file_path, ''));
            INSERT INTO nodes_fts(rowid, name, qualified_name, label, file_path)
            VALUES (new.row_id, new.name, new.qualified_name, new.label, coalesce(new.file_path, ''));
        END;

        INSERT INTO nodes_fts(nodes_fts) VALUES ('rebuild');
        ",
    ),
    (
        3,
        r"
        CREATE TABLE edit_journal (
            operation_id TEXT PRIMARY KEY COLLATE BINARY,
            record_version INTEGER NOT NULL DEFAULT 1 CHECK (record_version = 1),
            operation_kind TEXT NOT NULL COLLATE BINARY
                CHECK (operation_kind IN ('create', 'update', 'delete')),
            project_id TEXT NOT NULL COLLATE BINARY,
            path TEXT NOT NULL COLLATE BINARY,
            original_hash TEXT CHECK (original_hash IS NULL OR length(original_hash) = 64),
            new_hash TEXT CHECK (new_hash IS NULL OR length(new_hash) = 64),
            temp_path TEXT COLLATE BINARY,
            backup_path TEXT COLLATE BINARY,
            created_parent_paths_json TEXT NOT NULL DEFAULT '[]'
                CHECK (json_valid(created_parent_paths_json)
                       AND json_type(created_parent_paths_json) = 'array'),
            phase TEXT NOT NULL DEFAULT 'prepared' COLLATE BINARY
                CHECK (phase IN (
                    'prepared', 'backup_ready', 'replaced', 'indexed',
                    'committed', 'rolled_back'
                )),
            created_at TEXT NOT NULL
                DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at TEXT NOT NULL
                DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            last_error TEXT,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            CHECK (
                (operation_kind = 'create' AND original_hash IS NULL AND new_hash IS NOT NULL)
                OR
                (operation_kind = 'update' AND original_hash IS NOT NULL AND new_hash IS NOT NULL)
                OR
                (operation_kind = 'delete' AND original_hash IS NOT NULL AND new_hash IS NULL)
            )
        ) STRICT, WITHOUT ROWID;

        CREATE INDEX edit_journal_project_path_idx
            ON edit_journal(project_id, path, created_at, operation_id);
        CREATE INDEX edit_journal_incomplete_idx
            ON edit_journal(updated_at, operation_id)
            WHERE phase NOT IN ('committed', 'rolled_back');
        CREATE UNIQUE INDEX edit_journal_active_target_idx
            ON edit_journal(project_id, path)
            WHERE phase NOT IN ('committed', 'rolled_back');
        ",
    ),
    (
        4,
        r"
        CREATE TABLE project_summaries (
            project_id TEXT PRIMARY KEY COLLATE BINARY,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL
                DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at TEXT NOT NULL
                DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;

        CREATE TABLE runtime_traces (
            project_id TEXT NOT NULL COLLATE BINARY,
            caller TEXT NOT NULL COLLATE BINARY CHECK (length(caller) > 0),
            callee TEXT NOT NULL COLLATE BINARY CHECK (length(callee) > 0),
            count INTEGER NOT NULL CHECK (count > 0),
            created_at TEXT NOT NULL
                DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at TEXT NOT NULL
                DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            PRIMARY KEY (project_id, caller, callee),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;

        CREATE INDEX runtime_traces_project_count_idx
            ON runtime_traces(project_id, count DESC, caller, callee);
        ",
    ),
    (
        5,
        r"
        CREATE TABLE git_file_history (
            project_id TEXT NOT NULL COLLATE BINARY,
            path TEXT NOT NULL COLLATE BINARY,
            change_count INTEGER NOT NULL CHECK (change_count > 0),
            last_modified INTEGER NOT NULL CHECK (last_modified >= 0),
            PRIMARY KEY (project_id, path),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;

        CREATE TABLE git_cochanges (
            project_id TEXT NOT NULL COLLATE BINARY,
            file_a TEXT NOT NULL COLLATE BINARY,
            file_b TEXT NOT NULL COLLATE BINARY,
            co_changes INTEGER NOT NULL CHECK (co_changes > 0),
            coupling_score REAL NOT NULL CHECK (coupling_score >= 0 AND coupling_score <= 1),
            last_co_change INTEGER NOT NULL CHECK (last_co_change >= 0),
            PRIMARY KEY (project_id, file_a, file_b),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            CHECK (file_a < file_b)
        ) STRICT, WITHOUT ROWID;

        CREATE INDEX git_file_history_recent_idx
            ON git_file_history(project_id, last_modified DESC, path);
        CREATE INDEX git_cochanges_file_b_idx
            ON git_cochanges(project_id, file_b, file_a);
        CREATE INDEX git_cochanges_score_idx
            ON git_cochanges(project_id, coupling_score DESC, file_a, file_b);
        ",
    ),
];

pub fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
             version INTEGER PRIMARY KEY CHECK (version > 0),\
             applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP\
         ) STRICT;",
    )?;

    let applied: BTreeSet<u32> = {
        let mut statement = transaction.prepare("SELECT version FROM schema_migrations")?;
        let values = statement.query_map([], |row| row.get::<_, u32>(0))?;
        values.collect::<Result<_, _>>()?
    };
    if let Some(version) = applied.iter().next_back().copied()
        && version > CURRENT_SCHEMA_VERSION
    {
        return Err(StoreError::SchemaTooNew {
            actual: version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }

    for (version, sql) in MIGRATIONS {
        if applied.contains(version) {
            continue;
        }
        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version) VALUES (?1)",
            params![version],
        )?;
    }
    transaction.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

pub fn inspect(connection: &Connection) -> Result<SchemaInfo, StoreError> {
    let version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let mut statement = connection.prepare(
        "SELECT name, type FROM sqlite_schema \
         WHERE type IN ('table', 'index') AND name NOT LIKE 'sqlite_%' \
         ORDER BY type, name",
    )?;
    let entries = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut tables = BTreeSet::new();
    let mut indexes = BTreeSet::new();
    for entry in entries {
        let (name, kind) = entry?;
        if kind == "table" {
            tables.insert(name);
        } else {
            indexes.insert(name);
        }
    }
    Ok(SchemaInfo {
        version,
        fts5_enabled: tables.contains("nodes_fts"),
        tables,
        indexes,
    })
}
