use std::collections::BTreeSet;

use rusqlite::{Connection, TransactionBehavior, params};

use crate::{SchemaInfo, StoreError};

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

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
