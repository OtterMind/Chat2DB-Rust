use rusqlite::{OptionalExtension, params};

use crate::{Storage, StorageError, now_millis};

const MAX_DATASOURCE_ID_BYTES: usize = 512;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_ER_POSITION_BYTES: usize = 16 * 1024 * 1024;

impl Storage {
    /// Persists one pinned `MySQL` table idempotently.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource foreign-key, clock, or `SQLite` failures.
    pub fn pin_mysql_table(
        &self,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<(), StorageError> {
        validate_scope(datasource_id, database_name, schema_name)?;
        validate_table_name(table_name)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO mysql_pinned_tables (
                datasource_id, database_name, schema_name, table_name, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (datasource_id, database_name, schema_name, table_name) DO NOTHING",
            params![
                datasource_id,
                database_name,
                schema_name,
                table_name,
                now_millis()?
            ],
        )?;
        Ok(())
    }

    /// Removes one pinned `MySQL` table idempotently.
    ///
    /// # Errors
    ///
    /// Returns validation or `SQLite` failures.
    pub fn unpin_mysql_table(
        &self,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<(), StorageError> {
        validate_scope(datasource_id, database_name, schema_name)?;
        validate_table_name(table_name)?;
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM mysql_pinned_tables
             WHERE datasource_id = ?1 AND database_name = ?2
               AND schema_name = ?3 AND table_name = ?4",
            params![datasource_id, database_name, schema_name, table_name],
        )?;
        Ok(())
    }

    /// Lists pinned table names for one `MySQL` database/schema scope.
    ///
    /// # Errors
    ///
    /// Returns validation or `SQLite` failures.
    pub fn list_mysql_pinned_tables(
        &self,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
    ) -> Result<Vec<String>, StorageError> {
        validate_scope(datasource_id, database_name, schema_name)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT table_name FROM mysql_pinned_tables
             WHERE datasource_id = ?1 AND database_name = ?2 AND schema_name = ?3
             ORDER BY created_at_ms, table_name",
        )?;
        statement
            .query_map(params![datasource_id, database_name, schema_name], |row| {
                row.get(0)
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Reads the saved ER layout for one `MySQL` database/schema scope.
    ///
    /// # Errors
    ///
    /// Returns validation or `SQLite` failures.
    pub fn mysql_er_position(
        &self,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
    ) -> Result<Option<String>, StorageError> {
        validate_scope(datasource_id, database_name, schema_name)?;
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT position FROM mysql_er_positions
                 WHERE datasource_id = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![datasource_id, database_name, schema_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Inserts or replaces the saved ER layout for one `MySQL` database/schema scope.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource foreign-key, clock, or `SQLite` failures.
    pub fn save_mysql_er_position(
        &self,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
        position: &str,
    ) -> Result<(), StorageError> {
        validate_scope(datasource_id, database_name, schema_name)?;
        if position.len() > MAX_ER_POSITION_BYTES {
            return Err(StorageError::InvalidWorkspace(
                "ER position exceeds the 16 MiB UTF-8 limit",
            ));
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO mysql_er_positions (
                datasource_id, database_name, schema_name, position, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (datasource_id, database_name, schema_name) DO UPDATE SET
                position = excluded.position,
                updated_at_ms = excluded.updated_at_ms",
            params![
                datasource_id,
                database_name,
                schema_name,
                position,
                now_millis()?
            ],
        )?;
        Ok(())
    }
}

fn validate_scope(
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
) -> Result<(), StorageError> {
    if datasource_id.trim().is_empty() || datasource_id.len() > MAX_DATASOURCE_ID_BYTES {
        return Err(StorageError::InvalidWorkspace(
            "datasource id must be non-empty and at most 512 UTF-8 bytes",
        ));
    }
    validate_identifier(database_name, "database name")?;
    validate_identifier(schema_name, "schema name")
}

fn validate_table_name(table_name: &str) -> Result<(), StorageError> {
    if table_name.is_empty() || table_name.len() > MAX_IDENTIFIER_BYTES || table_name.contains('\0')
    {
        return Err(StorageError::InvalidWorkspace(
            "table name must be non-empty, NUL-free, and at most 256 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), StorageError> {
    if value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(StorageError::InvalidWorkspace(match field {
            "database name" => "database name must be NUL-free and at most 256 UTF-8 bytes",
            _ => "schema name must be NUL-free and at most 256 UTF-8 bytes",
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use crate::{CreateDatasource, SecretRef, SecretValue, SecretVault, SecretVaultError, Storage};

    #[derive(Debug)]
    struct EmptyVault;

    impl SecretVault for EmptyVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            _reference: &SecretRef,
            _value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn get(&self, _reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(None)
        }

        fn delete(&self, _reference: &SecretRef) -> Result<(), SecretVaultError> {
            Ok(())
        }
    }

    fn open(directory: &TempDir) -> Storage {
        Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens")
    }

    fn datasource(storage: &Storage) -> String {
        storage
            .create_datasource(
                CreateDatasource {
                    name: "Local MySQL".to_owned(),
                    driver_id: "mysql".to_owned(),
                },
                None,
            )
            .expect("datasource creates")
            .id
    }

    #[test]
    fn pins_are_idempotent_scoped_and_restart_safe() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let datasource_id = datasource(&storage);
        storage
            .pin_mysql_table(&datasource_id, "shop", "", "orders")
            .expect("pin creates");
        storage
            .pin_mysql_table(&datasource_id, "shop", "", "orders")
            .expect("duplicate pin is idempotent");
        storage
            .pin_mysql_table(&datasource_id, "shop", "", "users")
            .expect("second pin creates");
        drop(storage);

        let reopened = open(&directory);
        assert_eq!(
            reopened
                .list_mysql_pinned_tables(&datasource_id, "shop", "")
                .expect("pins list"),
            vec!["orders", "users"]
        );
        reopened
            .unpin_mysql_table(&datasource_id, "shop", "", "orders")
            .expect("pin deletes");
        assert_eq!(
            reopened
                .list_mysql_pinned_tables(&datasource_id, "shop", "")
                .expect("pins relist"),
            vec!["users"]
        );
    }

    #[test]
    fn er_position_upsert_replaces_the_existing_layout_and_survives_restart() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let datasource_id = datasource(&storage);
        storage
            .save_mysql_er_position(&datasource_id, "shop", "", r#"{"version":1}"#)
            .expect("first layout saves");
        storage
            .save_mysql_er_position(&datasource_id, "shop", "", r#"{"version":2}"#)
            .expect("second layout replaces first");
        drop(storage);

        let reopened = open(&directory);
        assert_eq!(
            reopened
                .mysql_er_position(&datasource_id, "shop", "")
                .expect("layout reads")
                .as_deref(),
            Some(r#"{"version":2}"#)
        );
    }
}
