use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use chat2db_contract::{
    ApiError, CommunitySchemaDiffEndpoint, CommunitySchemaDiffRequest, CommunitySchemaDiffSql,
};
use mysql_async::{Conn, Error as MysqlError, prelude::Queryable};

use crate::{
    AppError, AppErrorKind, Application,
    native_mysql::{finish_connection, open_resolved_connection, resolve_native_connection},
};

const SCHEMA_DIFF_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_SCHEMA_DIFF_OBJECTS: usize = 2_048;
const MAX_TABLE_DDL_BYTES: usize = 4 * 1024 * 1024;
const MAX_VIEW_DEFINITION_BYTES: usize = 4 * 1024 * 1024;
const MAX_SCHEMA_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const MAX_SCHEMA_DIFF_SQL_BYTES: usize = 16 * 1024 * 1024;
const NO_DIFFERENCES_SQL: &str = "-- No differences. ";

#[derive(Debug, Default)]
struct SchemaSnapshot {
    database_name: String,
    lower_case_table_names: u8,
    tables: BTreeMap<String, TableSnapshot>,
    views: BTreeMap<String, ViewSnapshot>,
}

#[derive(Debug)]
struct ViewSnapshot {
    definition: String,
}

#[derive(Debug)]
struct TableSnapshot {
    create_sql_without_foreign_keys: String,
    columns: Vec<ColumnDefinition>,
    indexes: Vec<IndexDefinition>,
    foreign_keys: Vec<ForeignKeyDefinition>,
    options: TableOptions,
}

#[derive(Debug)]
struct ColumnDefinition {
    name: String,
    sql: String,
    comparison_sql: String,
}

#[derive(Debug)]
struct IndexDefinition {
    name: String,
    sql: String,
    primary: bool,
}

#[derive(Debug)]
struct ForeignKeyDefinition {
    name: String,
    sql: String,
    referenced_table: String,
}

/// Pinned Community parity intentionally compares only these existing-table options.
///
/// CHECK constraints, partition definitions, and additional `MySQL` table options are preserved
/// when a missing table is created, but the Community schema-diff contract does not alter them on
/// an existing table. The SHOW CREATE `AUTO_INCREMENT=N` next counter is runtime state and is
/// deliberately excluded from both comparison and generated CREATE statements.
#[derive(Debug, Default)]
struct TableOptions {
    engine: Option<String>,
    charset: Option<String>,
    collation: Option<String>,
    comment: Option<String>,
}

#[derive(Debug, Default)]
struct ExistingTableDiff {
    foreign_key_drops: Vec<String>,
    table_changes: Vec<String>,
    foreign_key_adds: Vec<String>,
}

impl Application {
    /// Previews SQL that changes the target `MySQL` namespace to match the source.
    ///
    /// This method only reads metadata. Generated SQL is never executed automatically.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, connection, metadata, parse, resource-limit, or cleanup
    /// errors.
    pub async fn preview_mysql_schema_diff(
        &self,
        request: &CommunitySchemaDiffRequest,
    ) -> Result<CommunitySchemaDiffSql, AppError> {
        validate_endpoint(&request.source, "source")?;
        validate_endpoint(&request.target, "target")?;

        let source = load_endpoint_snapshot(self, &request.source).await?;
        let target = load_endpoint_snapshot(self, &request.target).await?;
        build_schema_diff(&source, &target).map(CommunitySchemaDiffSql::new)
    }
}

fn validate_endpoint(
    endpoint: &CommunitySchemaDiffEndpoint,
    role: &'static str,
) -> Result<(), AppError> {
    if endpoint.datasource_id.trim().is_empty() {
        return Err(invalid_schema_diff(format!(
            "The {role} datasource id is required"
        )));
    }
    if endpoint.database_name.trim().is_empty() {
        return Err(invalid_schema_diff(format!(
            "The {role} database name is required"
        )));
    }
    if endpoint.database_name.contains('\0') {
        return Err(invalid_schema_diff(format!(
            "The {role} database name cannot contain NUL"
        )));
    }
    Ok(())
}

async fn load_endpoint_snapshot(
    application: &Application,
    endpoint: &CommunitySchemaDiffEndpoint,
) -> Result<SchemaSnapshot, AppError> {
    let resolved = resolve_native_connection(application, &endpoint.datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result = tokio::time::timeout(
        SCHEMA_DIFF_TIMEOUT,
        load_schema_snapshot(&mut conn, &endpoint.database_name),
    )
    .await
    .map_err(|_| {
        AppError::unavailable(
            "mysql_schema_diff_timeout",
            "The MySQL schema diff metadata query timed out",
        )
    })?;
    finish_connection(conn, result).await
}

#[allow(clippy::too_many_lines)]
async fn load_schema_snapshot(
    conn: &mut Conn,
    database_name: &str,
) -> Result<SchemaSnapshot, AppError> {
    let database_exists = conn
        .exec_first::<u8, _, _>(
            "SELECT 1 FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ? LIMIT 1",
            (database_name,),
        )
        .await
        .map_err(schema_diff_query_error)?;
    if database_exists != Some(1) {
        return Err(AppError::not_found(
            "mysql_schema_diff_database_not_found",
            "The selected MySQL database does not exist",
        ));
    }

    let lower_case_table_names = conn
        .query_first::<u8, _>("SELECT @@lower_case_table_names")
        .await
        .map_err(schema_diff_query_error)?
        .ok_or_else(malformed_show_create)?;

    let table_query = format!(
        "SELECT TABLE_NAME FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE' \
         ORDER BY TABLE_NAME LIMIT {}",
        MAX_SCHEMA_DIFF_OBJECTS + 1
    );
    let table_names = conn
        .exec::<String, _, _>(table_query, (database_name,))
        .await
        .map_err(schema_diff_query_error)?;
    if table_names.len() > MAX_SCHEMA_DIFF_OBJECTS {
        return Err(schema_diff_resource_limit(
            "The selected MySQL database contains too many tables to diff safely",
        ));
    }

    let view_query = format!(
        "SELECT TABLE_NAME, VIEW_DEFINITION FROM information_schema.VIEWS \
         WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME LIMIT {}",
        MAX_SCHEMA_DIFF_OBJECTS + 1
    );
    let view_rows = conn
        .exec::<(String, Option<String>), _, _>(view_query, (database_name,))
        .await
        .map_err(schema_diff_query_error)?;
    if table_names.len().saturating_add(view_rows.len()) > MAX_SCHEMA_DIFF_OBJECTS {
        return Err(schema_diff_resource_limit(
            "The selected MySQL database contains too many objects to diff safely",
        ));
    }

    let mut snapshot = SchemaSnapshot {
        database_name: database_name.to_owned(),
        lower_case_table_names,
        ..SchemaSnapshot::default()
    };
    let mut snapshot_bytes = 0_usize;
    for table_name in table_names {
        let qualified_name = format!(
            "{}.{}",
            quote_identifier(database_name),
            quote_identifier(&table_name)
        );
        let row = conn
            .query_first::<(String, String), _>(format!("SHOW CREATE TABLE {qualified_name}"))
            .await
            .map_err(schema_diff_query_error)?
            .ok_or_else(malformed_show_create)?;
        let ddl = row.1;
        if ddl.len() > MAX_TABLE_DDL_BYTES {
            return Err(schema_diff_resource_limit(
                "A MySQL table definition is too large to diff safely",
            ));
        }
        snapshot_bytes = snapshot_bytes.checked_add(ddl.len()).ok_or_else(|| {
            schema_diff_resource_limit("The MySQL schema snapshot is too large to diff safely")
        })?;
        if snapshot_bytes > MAX_SCHEMA_SNAPSHOT_BYTES {
            return Err(schema_diff_resource_limit(
                "The MySQL schema snapshot is too large to diff safely",
            ));
        }
        snapshot
            .tables
            .insert(table_name.clone(), parse_table_snapshot(&table_name, &ddl)?);
    }
    for (view_name, definition) in view_rows {
        let definition = definition.ok_or_else(malformed_view_definition)?;
        if definition.len() > MAX_VIEW_DEFINITION_BYTES {
            return Err(schema_diff_resource_limit(
                "A MySQL view definition is too large to diff safely",
            ));
        }
        snapshot_bytes = snapshot_bytes
            .checked_add(definition.len())
            .ok_or_else(|| {
                schema_diff_resource_limit("The MySQL schema snapshot is too large to diff safely")
            })?;
        if snapshot_bytes > MAX_SCHEMA_SNAPSHOT_BYTES {
            return Err(schema_diff_resource_limit(
                "The MySQL schema snapshot is too large to diff safely",
            ));
        }
        snapshot.views.insert(
            view_name,
            ViewSnapshot {
                definition: definition.trim().to_owned(),
            },
        );
    }
    Ok(snapshot)
}

fn parse_table_snapshot(table_name: &str, ddl: &str) -> Result<TableSnapshot, AppError> {
    let (prefix, body, suffix) = create_table_parts(ddl).ok_or_else(malformed_show_create)?;
    let mut columns: Vec<ColumnDefinition> = Vec::new();
    let mut indexes: Vec<IndexDefinition> = Vec::new();
    let mut foreign_keys: Vec<ForeignKeyDefinition> = Vec::new();
    let mut definitions_without_foreign_keys = Vec::new();

    for definition in split_top_level_definitions(body) {
        let definition = definition.trim();
        if definition.is_empty() {
            continue;
        }
        if definition.starts_with('`') {
            let (name, _) =
                parse_quoted_identifier(definition).ok_or_else(malformed_show_create)?;
            if columns
                .iter()
                .any(|column| names_equal(&column.name, &name))
            {
                return Err(malformed_show_create());
            }
            columns.push(ColumnDefinition {
                name,
                sql: definition.to_owned(),
                comparison_sql: canonicalize_column_definition(definition),
            });
            definitions_without_foreign_keys.push(definition);
            continue;
        }
        if let Some(foreign_key) = parse_foreign_key_definition(definition)? {
            if foreign_keys
                .iter()
                .any(|existing| names_equal(&existing.name, &foreign_key.name))
            {
                return Err(malformed_show_create());
            }
            foreign_keys.push(foreign_key);
            continue;
        }
        if let Some(index) = parse_index_definition(definition)? {
            if indexes
                .iter()
                .any(|existing| names_equal(&existing.name, &index.name))
            {
                return Err(malformed_show_create());
            }
            indexes.push(index);
        }
        // Pinned Community parity preserves CHECK and other table-level clauses for new tables,
        // but does not claim to diff them on an existing table.
        definitions_without_foreign_keys.push(definition);
    }

    if columns.is_empty() {
        return Err(AppError::unavailable(
            "mysql_schema_diff_metadata_invalid",
            format!("MySQL returned an invalid definition for table {table_name}"),
        ));
    }
    let suffix_without_runtime_counter = strip_table_option(suffix, "AUTO_INCREMENT");
    let create_sql_without_foreign_keys = format!(
        "{}\n  {}\n{}",
        prefix.trim_end(),
        definitions_without_foreign_keys.join(",\n  "),
        suffix_without_runtime_counter
            .trim_start()
            .trim_end_matches(';')
    );
    Ok(TableSnapshot {
        create_sql_without_foreign_keys,
        columns,
        indexes,
        foreign_keys,
        options: parse_table_options(suffix)?,
    })
}

fn parse_foreign_key_definition(
    definition: &str,
) -> Result<Option<ForeignKeyDefinition>, AppError> {
    let Some(rest) = strip_ascii_prefix(definition, "CONSTRAINT") else {
        return Ok(None);
    };
    let rest = rest.trim_start();
    let (name, consumed) = parse_quoted_identifier(rest).ok_or_else(malformed_show_create)?;
    if !has_ascii_prefix(rest[consumed..].trim_start(), "FOREIGN KEY") {
        return Ok(None);
    }
    let referenced_table = parse_referenced_table(definition).ok_or_else(malformed_show_create)?;
    Ok(Some(ForeignKeyDefinition {
        name,
        sql: definition.to_owned(),
        referenced_table,
    }))
}

fn parse_referenced_table(definition: &str) -> Option<String> {
    const REFERENCES: &str = "REFERENCES";
    let start = find_unquoted_keyword(definition, REFERENCES)? + REFERENCES.len();
    let rest = definition.get(start..)?.trim_start();
    let (first, consumed) = parse_quoted_identifier(rest)?;
    let after_first = rest.get(consumed..)?.trim_start();
    let Some(after_dot) = after_first.strip_prefix('.') else {
        return Some(first);
    };
    parse_quoted_identifier(after_dot.trim_start()).map(|(table, _)| table)
}

fn parse_index_definition(definition: &str) -> Result<Option<IndexDefinition>, AppError> {
    if has_ascii_prefix(definition, "PRIMARY KEY") {
        return Ok(Some(IndexDefinition {
            name: "PRIMARY".to_owned(),
            sql: definition.to_owned(),
            primary: true,
        }));
    }
    for prefix in ["UNIQUE KEY", "FULLTEXT KEY", "SPATIAL KEY", "KEY"] {
        let Some(rest) = strip_ascii_prefix(definition, prefix) else {
            continue;
        };
        let (name, _) =
            parse_quoted_identifier(rest.trim_start()).ok_or_else(malformed_show_create)?;
        return Ok(Some(IndexDefinition {
            name,
            sql: definition.to_owned(),
            primary: false,
        }));
    }
    Ok(None)
}

fn parse_table_options(suffix: &str) -> Result<TableOptions, AppError> {
    let engine = parse_identifier_table_option(suffix, "ENGINE")?;
    let charset = parse_identifier_table_option(suffix, "CHARSET")?;
    let collation = parse_identifier_table_option(suffix, "COLLATE")?;
    let comment = table_option_value(suffix, "COMMENT")
        .map(|value| {
            if quoted_value_end(value) == Some(value.len()) {
                Ok(value.to_owned())
            } else {
                Err(malformed_show_create())
            }
        })
        .transpose()?;
    Ok(TableOptions {
        engine,
        charset,
        collation,
        comment,
    })
}

fn strip_table_option(suffix: &str, name: &str) -> String {
    let Some(name_start) = find_unquoted_keyword(suffix, name) else {
        return suffix.to_owned();
    };
    let bytes = suffix.as_bytes();
    let mut value_start = skip_ascii_whitespace(bytes, name_start + name.len());
    if bytes.get(value_start) == Some(&b'=') {
        value_start = skip_ascii_whitespace(bytes, value_start + 1);
    }
    let value_end = if matches!(bytes.get(value_start), Some(b'\'' | b'"')) {
        suffix
            .get(value_start..)
            .and_then(quoted_value_end)
            .map_or(value_start, |length| value_start + length)
    } else {
        let mut end = value_start;
        while bytes
            .get(end)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b',')
        {
            end += 1;
        }
        end
    };
    if value_end == value_start {
        return suffix.to_owned();
    }
    let before = suffix[..name_start].trim_end();
    let after = suffix[value_end..].trim_start();
    match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_owned(),
        (_, true) => before.to_owned(),
        (false, false) => format!("{before} {after}"),
    }
}

fn parse_identifier_table_option(suffix: &str, name: &str) -> Result<Option<String>, AppError> {
    table_option_value(suffix, name)
        .map(|value| {
            if value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                Ok(value.to_owned())
            } else {
                Err(malformed_show_create())
            }
        })
        .transpose()
}

fn table_option_value<'a>(suffix: &'a str, name: &str) -> Option<&'a str> {
    let name_start = find_unquoted_keyword(suffix, name)?;
    let bytes = suffix.as_bytes();
    let mut value_start = skip_ascii_whitespace(bytes, name_start + name.len());
    if bytes.get(value_start) == Some(&b'=') {
        value_start = skip_ascii_whitespace(bytes, value_start + 1);
    }
    if matches!(bytes.get(value_start), Some(b'\'' | b'"')) {
        let value = suffix.get(value_start..)?;
        let value_end = quoted_value_end(value)?;
        return value.get(..value_end);
    }
    let mut value_end = value_start;
    while bytes
        .get(value_end)
        .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b',')
    {
        value_end += 1;
    }
    (value_end > value_start).then(|| &suffix[value_start..value_end])
}

fn find_unquoted_keyword(value: &str, keyword: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut quote = None;
    let mut index = 0_usize;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if bytes[index] == b'\\' && matches!(delimiter, b'\'' | b'"') {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if bytes[index] == delimiter {
                if bytes.get(index + 1) == Some(&delimiter) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            quote = Some(bytes[index]);
            index += 1;
            continue;
        }
        let end = index.checked_add(keyword.len())?;
        let matches = value
            .get(index..end)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(keyword));
        let boundary_before = index == 0
            || bytes
                .get(index - 1)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_');
        let boundary_after = bytes
            .get(end)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_');
        if matches && boundary_before && boundary_after {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn quoted_value_end(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let delimiter = *bytes.first()?;
    if !matches!(delimiter, b'\'' | b'"') {
        return None;
    }
    let mut index = 1_usize;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
            continue;
        }
        if bytes[index] == delimiter {
            if bytes.get(index + 1) == Some(&delimiter) {
                index += 2;
                continue;
            }
            return Some(index + 1);
        }
        index += 1;
    }
    None
}

#[allow(clippy::too_many_lines)]
fn build_schema_diff(source: &SchemaSnapshot, target: &SchemaSnapshot) -> Result<String, AppError> {
    validate_case_only_object_conflicts(source, target)?;
    let source_view_order = topologically_order_views(source)?;
    let target_view_order = topologically_order_views(target)?;
    let mut view_drops = Vec::new();
    let mut foreign_key_drops = Vec::new();
    let mut table_changes = Vec::new();
    let mut foreign_key_adds = Vec::new();
    let mut view_upserts = Vec::new();
    let mut changed_tables = BTreeSet::new();
    let mut existing_diffs = BTreeMap::new();

    for view_name in target_view_order.iter().rev() {
        if !source.views.contains_key(view_name) {
            view_drops.push(format!(
                "DROP VIEW {}",
                qualified_object(&target.database_name, view_name)
            ));
        }
    }

    for (table_name, source_table) in &source.tables {
        match target.tables.get(table_name) {
            None => {
                changed_tables.insert(table_name.clone());
                table_changes.push(qualify_create_table_sql(
                    &source_table.create_sql_without_foreign_keys,
                    &target.database_name,
                    table_name,
                )?);
                foreign_key_adds.extend(add_all_foreign_keys(
                    table_name,
                    source_table,
                    &source.database_name,
                    &target.database_name,
                ));
            }
            Some(target_table) => {
                let diff = diff_existing_table(
                    table_name,
                    source_table,
                    target_table,
                    &source.database_name,
                    &target.database_name,
                );
                if !diff.table_changes.is_empty() {
                    changed_tables.insert(table_name.clone());
                }
                existing_diffs.insert(table_name.clone(), diff);
            }
        }
    }
    for (table_name, target_table) in &target.tables {
        if !source.tables.contains_key(table_name) {
            changed_tables.insert(table_name.clone());
            foreign_key_drops.extend(drop_all_foreign_keys(
                table_name,
                target_table,
                &target.database_name,
            ));
            table_changes.push(format!(
                "DROP TABLE {}",
                qualified_object(&target.database_name, table_name)
            ));
        }
    }

    for (table_name, diff) in &mut existing_diffs {
        let source_table = source
            .tables
            .get(table_name)
            .expect("existing source table was collected above");
        let target_table = target
            .tables
            .get(table_name)
            .expect("existing target table was collected above");
        let owner_changed = changed_tables.contains(table_name);
        for foreign_key in &target_table.foreign_keys {
            if owner_changed || changed_tables.contains(&foreign_key.referenced_table) {
                push_unique(
                    &mut diff.foreign_key_drops,
                    format!(
                        "ALTER TABLE {} DROP FOREIGN KEY {}",
                        qualified_object(&target.database_name, table_name),
                        quote_identifier(&foreign_key.name)
                    ),
                );
            }
        }
        for foreign_key in &source_table.foreign_keys {
            if owner_changed || changed_tables.contains(&foreign_key.referenced_table) {
                push_unique(
                    &mut diff.foreign_key_adds,
                    format!(
                        "ALTER TABLE {} ADD {}",
                        qualified_object(&target.database_name, table_name),
                        retarget_foreign_key_sql(
                            &foreign_key.sql,
                            &source.database_name,
                            &target.database_name
                        )
                    ),
                );
            }
        }
    }
    for diff in existing_diffs.into_values() {
        foreign_key_drops.extend(diff.foreign_key_drops);
        table_changes.extend(diff.table_changes);
        foreign_key_adds.extend(diff.foreign_key_adds);
    }

    for view_name in source_view_order {
        let source_view = source
            .views
            .get(&view_name)
            .expect("the view dependency order only contains source views");
        let target_definition = rewrite_qualified_catalog(
            &source_view.definition,
            &source.database_name,
            &target.database_name,
        );
        let statement = match target.views.get(&view_name) {
            None => Some(format!(
                "CREATE VIEW {} AS {target_definition}",
                qualified_object(&target.database_name, &view_name)
            )),
            Some(target_view) if target_view.definition.trim() != target_definition => {
                Some(format!(
                    "CREATE OR REPLACE VIEW {} AS {target_definition}",
                    qualified_object(&target.database_name, &view_name)
                ))
            }
            Some(_) => None,
        };
        if let Some(statement) = statement {
            view_upserts.push(statement);
        }
    }

    let statements = view_drops
        .into_iter()
        .chain(foreign_key_drops)
        .chain(table_changes)
        .chain(foreign_key_adds)
        .chain(view_upserts)
        .collect::<Vec<_>>();
    render_statements(&statements)
}

fn validate_case_only_object_conflicts(
    source: &SchemaSnapshot,
    target: &SchemaSnapshot,
) -> Result<(), AppError> {
    if target.lower_case_table_names == 0 {
        return Ok(());
    }
    let source_names = source
        .tables
        .keys()
        .chain(source.views.keys())
        .collect::<Vec<_>>();
    let target_names = target
        .tables
        .keys()
        .chain(target.views.keys())
        .collect::<Vec<_>>();
    let source_has_collision = source_names.iter().enumerate().any(|(index, left)| {
        source_names[index + 1..]
            .iter()
            .any(|right| *left != *right && names_equal(left, right))
    });
    let source_target_conflict = source_names.iter().any(|source_name| {
        target_names.iter().any(|target_name| {
            *source_name != *target_name && names_equal(source_name, target_name)
        })
    });
    if source_has_collision || source_target_conflict {
        return Err(AppError::invalid(
            "mysql_schema_diff_case_conflict",
            "The target MySQL server uses case-insensitive object names and the schema contains a case-only name conflict",
        ));
    }
    Ok(())
}

fn topologically_order_views(snapshot: &SchemaSnapshot) -> Result<Vec<String>, AppError> {
    let mut dependency_count = snapshot
        .views
        .keys()
        .map(|name| (name.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut dependents = BTreeMap::<String, BTreeSet<String>>::new();
    for (view_name, view) in &snapshot.views {
        for dependency in qualified_catalog_objects(&view.definition, &snapshot.database_name) {
            let dependency = snapshot.views.keys().find(|candidate| {
                *candidate == &dependency
                    || (snapshot.lower_case_table_names != 0 && names_equal(candidate, &dependency))
            });
            let Some(dependency) = dependency else {
                continue;
            };
            if dependents
                .entry(dependency.clone())
                .or_default()
                .insert(view_name.clone())
            {
                *dependency_count
                    .get_mut(view_name)
                    .expect("every source view has a dependency counter") += 1;
            }
        }
    }

    let mut ready = dependency_count
        .iter()
        .filter_map(|(name, count)| (*count == 0).then_some(name.clone()))
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(snapshot.views.len());
    while let Some(view_name) = ready.pop_first() {
        ordered.push(view_name.clone());
        if let Some(children) = dependents.get(&view_name) {
            for child in children {
                let count = dependency_count
                    .get_mut(child)
                    .expect("every dependent view has a dependency counter");
                *count -= 1;
                if *count == 0 {
                    ready.insert(child.clone());
                }
            }
        }
    }
    if ordered.len() != snapshot.views.len() {
        return Err(AppError::invalid(
            "mysql_schema_diff_view_dependency_cycle",
            "The MySQL schema contains a view dependency cycle that cannot be migrated safely",
        ));
    }
    Ok(ordered)
}

fn qualified_catalog_objects(definition: &str, catalog: &str) -> BTreeSet<String> {
    let bytes = definition.as_bytes();
    let mut objects = BTreeSet::new();
    let mut index = 0_usize;
    while index < bytes.len() {
        if matches!(bytes[index], b'\'' | b'"') {
            let remainder = &definition[index..];
            let end = quoted_value_end(remainder).unwrap_or(remainder.len());
            index += end;
            continue;
        }
        if bytes[index] != b'`' {
            index += 1;
            continue;
        }
        let Some((identifier, consumed)) = parse_quoted_identifier(&definition[index..]) else {
            index += 1;
            continue;
        };
        let token_end = index + consumed;
        let dot = skip_ascii_whitespace(bytes, token_end);
        let object_start = skip_ascii_whitespace(bytes, dot.saturating_add(1));
        if identifier.eq_ignore_ascii_case(catalog)
            && bytes.get(dot) == Some(&b'.')
            && bytes.get(object_start) == Some(&b'`')
            && let Some((object, object_length)) =
                parse_quoted_identifier(&definition[object_start..])
        {
            objects.insert(object);
            index = object_start + object_length;
            continue;
        }
        index = token_end;
    }
    objects
}

fn qualify_create_table_sql(
    create_sql: &str,
    target_database: &str,
    table_name: &str,
) -> Result<String, AppError> {
    let Some(rest) = strip_ascii_prefix(create_sql.trim(), "CREATE TABLE") else {
        return Err(malformed_show_create());
    };
    let rest = rest.trim_start();
    let (parsed_name, consumed) =
        parse_quoted_identifier(rest).ok_or_else(malformed_show_create)?;
    if !names_equal(&parsed_name, table_name) {
        return Err(malformed_show_create());
    }
    Ok(format!(
        "CREATE TABLE {}{}",
        qualified_object(target_database, table_name),
        &rest[consumed..]
    ))
}

fn rewrite_qualified_catalog(definition: &str, source: &str, target: &str) -> String {
    if source.eq_ignore_ascii_case(target) {
        return definition.trim().to_owned();
    }
    let bytes = definition.as_bytes();
    let mut output = Vec::with_capacity(definition.len().saturating_add(target.len()));
    let mut index = 0_usize;
    while index < bytes.len() {
        if matches!(bytes[index], b'\'' | b'"') {
            let remainder = &definition[index..];
            let end = quoted_value_end(remainder).unwrap_or(remainder.len());
            output.extend_from_slice(&bytes[index..index + end]);
            index += end;
            continue;
        }
        if bytes[index] != b'`' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        let Some((identifier, consumed)) = parse_quoted_identifier(&definition[index..]) else {
            output.push(bytes[index]);
            index += 1;
            continue;
        };
        let token_end = index + consumed;
        let dot = skip_ascii_whitespace(bytes, token_end);
        if identifier.eq_ignore_ascii_case(source) && bytes.get(dot) == Some(&b'.') {
            output.extend_from_slice(quote_identifier(target).as_bytes());
        } else {
            output.extend_from_slice(&bytes[index..token_end]);
        }
        index = token_end;
    }
    String::from_utf8(output)
        .expect("rewriting a valid UTF-8 MySQL view definition must preserve UTF-8")
        .trim()
        .to_owned()
}

fn retarget_foreign_key_sql(definition: &str, source: &str, target: &str) -> String {
    const REFERENCES: &str = "REFERENCES";
    let Some(table_start) = find_unquoted_keyword(definition, REFERENCES)
        .map(|start| skip_ascii_whitespace(definition.as_bytes(), start + REFERENCES.len()))
    else {
        return definition.trim().to_owned();
    };
    let Some((_, consumed)) = parse_quoted_identifier(&definition[table_start..]) else {
        return definition.trim().to_owned();
    };
    let after_first = skip_ascii_whitespace(definition.as_bytes(), table_start + consumed);
    if definition.as_bytes().get(after_first) == Some(&b'.') {
        return rewrite_qualified_catalog(definition, source, target);
    }

    format!(
        "{}{}.{}",
        &definition[..table_start],
        quote_identifier(target),
        &definition[table_start..]
    )
    .trim()
    .to_owned()
}

fn canonicalize_foreign_key_sql(definition: &str, local_catalog: &str) -> String {
    const REFERENCES: &str = "REFERENCES";
    let Some(table_start) = find_unquoted_keyword(definition, REFERENCES)
        .map(|start| skip_ascii_whitespace(definition.as_bytes(), start + REFERENCES.len()))
    else {
        return definition.trim().to_owned();
    };
    let Some((catalog, consumed)) = parse_quoted_identifier(&definition[table_start..]) else {
        return definition.trim().to_owned();
    };
    let dot = skip_ascii_whitespace(definition.as_bytes(), table_start + consumed);
    if !catalog.eq_ignore_ascii_case(local_catalog) || definition.as_bytes().get(dot) != Some(&b'.')
    {
        return definition.trim().to_owned();
    }
    let object_start = skip_ascii_whitespace(definition.as_bytes(), dot + 1);
    if parse_quoted_identifier(&definition[object_start..]).is_none() {
        return definition.trim().to_owned();
    }
    format!(
        "{}{}",
        &definition[..table_start],
        &definition[object_start..]
    )
    .trim()
    .to_owned()
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

#[allow(clippy::too_many_lines)]
fn diff_existing_table(
    table_name: &str,
    source: &TableSnapshot,
    target: &TableSnapshot,
    source_database: &str,
    target_database: &str,
) -> ExistingTableDiff {
    let qualified_table = qualified_object(target_database, table_name);
    let mut diff = ExistingTableDiff::default();

    for target_foreign_key in &target.foreign_keys {
        let source_foreign_key = find_foreign_key(&source.foreign_keys, &target_foreign_key.name);
        let source_sql = source_foreign_key
            .map(|foreign_key| canonicalize_foreign_key_sql(&foreign_key.sql, source_database));
        let target_sql = canonicalize_foreign_key_sql(&target_foreign_key.sql, target_database);
        if source_sql.as_deref() != Some(target_sql.as_str()) {
            diff.foreign_key_drops.push(format!(
                "ALTER TABLE {qualified_table} DROP FOREIGN KEY {}",
                quote_identifier(&target_foreign_key.name)
            ));
        }
    }

    for target_index in target.indexes.iter().filter(|index| !index.primary) {
        let source_index = find_index(&source.indexes, &target_index.name);
        if source_index.is_none_or(|index| index.sql != target_index.sql) {
            diff.table_changes.push(format!(
                "ALTER TABLE {qualified_table} DROP INDEX {}",
                quote_identifier(&target_index.name)
            ));
        }
    }

    let mut current_order = target
        .columns
        .iter()
        .filter(|column| find_column(&source.columns, &column.name).is_some())
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    for (desired_position, source_column) in source.columns.iter().enumerate() {
        let target_column = find_column(&target.columns, &source_column.name);
        let current_position = current_order
            .iter()
            .position(|name| names_equal(name, &source_column.name));
        let position_changed = current_position != Some(desired_position);
        let position = column_position(&source.columns, desired_position);

        match target_column {
            None => diff.table_changes.push(format!(
                "ALTER TABLE {qualified_table} ADD COLUMN {}{position}",
                source_column.sql
            )),
            Some(target_column)
                if target_column.comparison_sql != source_column.comparison_sql
                    || position_changed =>
            {
                diff.table_changes.push(format!(
                    "ALTER TABLE {qualified_table} MODIFY COLUMN {}{position}",
                    source_column.sql
                ));
            }
            Some(_) => {}
        }

        if let Some(position) = current_position {
            current_order.remove(position);
        }
        current_order.insert(desired_position, source_column.name.clone());
    }

    for source_index in source.indexes.iter().filter(|index| !index.primary) {
        let target_index = find_index(&target.indexes, &source_index.name);
        if target_index.is_none_or(|index| index.sql != source_index.sql) {
            diff.table_changes.push(format!(
                "ALTER TABLE {qualified_table} ADD {}",
                source_index.sql
            ));
        }
    }

    let source_primary = source.indexes.iter().find(|index| index.primary);
    let target_primary = target.indexes.iter().find(|index| index.primary);
    match (source_primary, target_primary) {
        (Some(source_primary), Some(target_primary))
            if source_primary.sql != target_primary.sql =>
        {
            diff.table_changes.push(format!(
                "ALTER TABLE {qualified_table} DROP PRIMARY KEY, ADD {}",
                source_primary.sql
            ));
        }
        (Some(source_primary), None) => diff.table_changes.push(format!(
            "ALTER TABLE {qualified_table} ADD {}",
            source_primary.sql
        )),
        (None, Some(_)) => diff
            .table_changes
            .push(format!("ALTER TABLE {qualified_table} DROP PRIMARY KEY")),
        _ => {}
    }

    for target_column in &target.columns {
        if find_column(&source.columns, &target_column.name).is_none() {
            diff.table_changes.push(format!(
                "ALTER TABLE {qualified_table} DROP COLUMN {}",
                quote_identifier(&target_column.name)
            ));
        }
    }

    if let Some(statement) = diff_table_options(
        target_database,
        table_name,
        &source.options,
        &target.options,
    ) {
        diff.table_changes.push(statement);
    }
    for source_foreign_key in &source.foreign_keys {
        let target_foreign_key = find_foreign_key(&target.foreign_keys, &source_foreign_key.name);
        let source_comparison_sql =
            canonicalize_foreign_key_sql(&source_foreign_key.sql, source_database);
        let target_matches = target_foreign_key.is_some_and(|foreign_key| {
            canonicalize_foreign_key_sql(&foreign_key.sql, target_database) == source_comparison_sql
        });
        if !target_matches {
            let source_sql =
                retarget_foreign_key_sql(&source_foreign_key.sql, source_database, target_database);
            diff.foreign_key_adds
                .push(format!("ALTER TABLE {qualified_table} ADD {source_sql}"));
        }
    }
    diff
}

fn drop_all_foreign_keys(
    table_name: &str,
    table: &TableSnapshot,
    target_database: &str,
) -> Vec<String> {
    let qualified_table = qualified_object(target_database, table_name);
    table
        .foreign_keys
        .iter()
        .map(|foreign_key| {
            format!(
                "ALTER TABLE {qualified_table} DROP FOREIGN KEY {}",
                quote_identifier(&foreign_key.name)
            )
        })
        .collect()
}

fn add_all_foreign_keys(
    table_name: &str,
    table: &TableSnapshot,
    source_database: &str,
    target_database: &str,
) -> Vec<String> {
    let qualified_table = qualified_object(target_database, table_name);
    table
        .foreign_keys
        .iter()
        .map(|foreign_key| {
            let source_sql =
                retarget_foreign_key_sql(&foreign_key.sql, source_database, target_database);
            format!("ALTER TABLE {qualified_table} ADD {source_sql}")
        })
        .collect()
}

fn diff_table_options(
    target_database: &str,
    table_name: &str,
    source: &TableOptions,
    target: &TableOptions,
) -> Option<String> {
    let mut clauses = Vec::new();
    if !option_eq_ignore_ascii_case(source.engine.as_deref(), target.engine.as_deref()) {
        clauses.push(format!("ENGINE={}", source.engine.as_deref()?));
    }
    if !option_eq_ignore_ascii_case(source.charset.as_deref(), target.charset.as_deref())
        || !option_eq_ignore_ascii_case(source.collation.as_deref(), target.collation.as_deref())
    {
        if let Some(charset) = source.charset.as_deref() {
            clauses.push(format!("DEFAULT CHARACTER SET={charset}"));
        }
        if let Some(collation) = source.collation.as_deref() {
            clauses.push(format!("COLLATE={collation}"));
        }
    }
    if source.comment != target.comment {
        clauses.push(format!(
            "COMMENT={}",
            source.comment.as_deref().unwrap_or("''")
        ));
    }
    (!clauses.is_empty()).then(|| {
        format!(
            "ALTER TABLE {} {}",
            qualified_object(target_database, table_name),
            clauses.join(", ")
        )
    })
}

fn option_eq_ignore_ascii_case(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        (None, None) => true,
        _ => false,
    }
}

fn render_statements(statements: &[String]) -> Result<String, AppError> {
    if statements.is_empty() {
        return Ok(NO_DIFFERENCES_SQL.to_owned());
    }
    let required_bytes = statements.iter().try_fold(0_usize, |total, statement| {
        total.checked_add(statement.trim_end_matches(';').len() + 3)
    });
    let Some(required_bytes) = required_bytes else {
        return Err(schema_diff_resource_limit(
            "The generated MySQL schema diff is too large",
        ));
    };
    if required_bytes > MAX_SCHEMA_DIFF_SQL_BYTES {
        return Err(schema_diff_resource_limit(
            "The generated MySQL schema diff is too large",
        ));
    }
    let mut sql = String::with_capacity(required_bytes);
    for statement in statements {
        sql.push_str(statement.trim_end_matches(';'));
        sql.push_str(";\n\n");
    }
    Ok(sql)
}

fn column_position(columns: &[ColumnDefinition], position: usize) -> String {
    if position == 0 {
        " FIRST".to_owned()
    } else {
        format!(" AFTER {}", quote_identifier(&columns[position - 1].name))
    }
}

fn find_column<'a>(columns: &'a [ColumnDefinition], name: &str) -> Option<&'a ColumnDefinition> {
    columns
        .iter()
        .find(|column| names_equal(&column.name, name))
}

fn find_index<'a>(indexes: &'a [IndexDefinition], name: &str) -> Option<&'a IndexDefinition> {
    indexes.iter().find(|index| names_equal(&index.name, name))
}

fn find_foreign_key<'a>(
    foreign_keys: &'a [ForeignKeyDefinition],
    name: &str,
) -> Option<&'a ForeignKeyDefinition> {
    foreign_keys
        .iter()
        .find(|foreign_key| names_equal(&foreign_key.name, name))
}

fn names_equal(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn quote_identifier(value: &str) -> String {
    format!("`{}`", value.replace('`', "``"))
}

fn qualified_object(database_name: &str, object_name: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(database_name),
        quote_identifier(object_name)
    )
}

fn canonicalize_column_definition(definition: &str) -> String {
    let bytes = definition.as_bytes();
    let mut canonical = String::with_capacity(definition.len());
    let mut copied_until = 0_usize;
    let mut quote = None;
    let mut index = 0_usize;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if matches!(delimiter, b'\'' | b'"') && bytes[index] == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if bytes[index] == delimiter {
                if bytes.get(index + 1) == Some(&delimiter) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            quote = Some(bytes[index]);
            index += 1;
            continue;
        }
        let Some(after_character) = keyword_end(definition, index, "CHARACTER") else {
            index += 1;
            continue;
        };
        let after_character = skip_ascii_whitespace(bytes, after_character);
        let Some(after_set) = keyword_end(definition, after_character, "SET") else {
            index += 1;
            continue;
        };
        let charset_start = skip_ascii_whitespace(bytes, after_set);
        let mut charset_end = charset_start;
        while bytes
            .get(charset_end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            charset_end += 1;
        }
        if charset_end == charset_start {
            index += 1;
            continue;
        }
        let collate_start = skip_ascii_whitespace(bytes, charset_end);
        if keyword_end(definition, collate_start, "COLLATE").is_none() {
            index += 1;
            continue;
        }
        canonical.push_str(&definition[copied_until..index]);
        copied_until = collate_start;
        index = collate_start;
    }
    canonical.push_str(&definition[copied_until..]);
    canonical
}

fn keyword_end(value: &str, start: usize, keyword: &str) -> Option<usize> {
    let end = start.checked_add(keyword.len())?;
    let candidate = value.get(start..end)?;
    if !candidate.eq_ignore_ascii_case(keyword) {
        return None;
    }
    let bytes = value.as_bytes();
    let boundary_before = start == 0
        || bytes
            .get(start - 1)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_');
    let boundary_after = bytes
        .get(end)
        .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_');
    (boundary_before && boundary_after).then_some(end)
}

fn skip_ascii_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

fn parse_quoted_identifier(input: &str) -> Option<(String, usize)> {
    let mut characters = input.char_indices().peekable();
    if characters.next()?.1 != '`' {
        return None;
    }
    let mut value = String::new();
    while let Some((index, character)) = characters.next() {
        if character != '`' {
            value.push(character);
            continue;
        }
        if characters.peek().is_some_and(|(_, next)| *next == '`') {
            characters.next();
            value.push('`');
            continue;
        }
        return Some((value, index + character.len_utf8()));
    }
    None
}

fn has_ascii_prefix(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

fn strip_ascii_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    has_ascii_prefix(value, prefix).then(|| &value[prefix.len()..])
}

fn create_table_parts(ddl: &str) -> Option<(&str, &str, &str)> {
    let bytes = ddl.as_bytes();
    let mut quote = None;
    let mut open = None;
    let mut depth = 0_usize;
    let mut index = 0_usize;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if matches!(delimiter, b'\'' | b'"') && bytes[index] == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if bytes[index] == delimiter {
                if bytes.get(index + 1) == Some(&delimiter) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        match bytes[index] {
            b'\'' | b'"' | b'`' => quote = Some(bytes[index]),
            b'(' => {
                depth += 1;
                if open.is_none() {
                    open = Some(index + 1);
                }
            }
            b')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return open.map(|start| (&ddl[..start], &ddl[start..index], &ddl[index..]));
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn split_top_level_definitions(body: &str) -> Vec<&str> {
    let bytes = body.as_bytes();
    let mut definitions = Vec::new();
    let mut quote = None;
    let mut depth = 0_usize;
    let mut start = 0_usize;
    let mut index = 0_usize;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if matches!(delimiter, b'\'' | b'"') && bytes[index] == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if bytes[index] == delimiter {
                if bytes.get(index + 1) == Some(&delimiter) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        match bytes[index] {
            b'\'' | b'"' | b'`' => quote = Some(bytes[index]),
            b'(' => depth += 1,
            b')' if depth > 0 => depth -= 1,
            b',' if depth == 0 => {
                definitions.push(&body[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    definitions.push(&body[start..]);
    definitions
}

fn schema_diff_query_error(_error: MysqlError) -> AppError {
    AppError::unavailable(
        "mysql_schema_diff_query_failed",
        "The MySQL schema diff metadata could not be loaded",
    )
}

fn malformed_show_create() -> AppError {
    AppError::unavailable(
        "mysql_schema_diff_metadata_invalid",
        "MySQL returned a table definition that could not be compared safely",
    )
}

fn malformed_view_definition() -> AppError {
    AppError::unavailable(
        "mysql_schema_diff_metadata_invalid",
        "MySQL returned a view definition that could not be compared safely",
    )
}

fn invalid_schema_diff(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_community_schema_diff_request", message)
}

fn schema_diff_resource_limit(message: &'static str) -> AppError {
    AppError::new(
        AppErrorKind::ResourceExhausted,
        ApiError::new("mysql_schema_diff_resource_limit", message),
    )
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{CommunitySchemaDiffEndpoint, CommunitySchemaDiffRequest};

    use crate::Application;

    use super::{
        NO_DIFFERENCES_SQL, SchemaSnapshot, ViewSnapshot, build_schema_diff,
        canonicalize_column_definition, parse_table_snapshot, rewrite_qualified_catalog,
    };

    fn snapshot(database_name: &str) -> SchemaSnapshot {
        SchemaSnapshot {
            database_name: database_name.to_owned(),
            ..SchemaSnapshot::default()
        }
    }

    #[test]
    fn no_difference_uses_the_pinned_community_comment() {
        let mut snapshot = snapshot("same_db");
        snapshot.tables.insert(
            "items".to_owned(),
            parse_table_snapshot(
                "items",
                "CREATE TABLE `items` (\n  `id` bigint NOT NULL,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB",
            )
            .expect("table ddl"),
        );

        assert_eq!(
            build_schema_diff(&snapshot, &snapshot).expect("no diff"),
            NO_DIFFERENCES_SQL
        );
    }

    #[test]
    fn table_column_order_and_index_changes_generate_target_migration_sql() {
        let mut source = snapshot("source_db");
        source.tables.insert(
            "added".to_owned(),
            parse_table_snapshot(
                "added",
                "CREATE TABLE `added` (\n  `id` bigint NOT NULL,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB",
            )
            .expect("added table"),
        );
        source.tables.insert(
            "changed".to_owned(),
            parse_table_snapshot(
                "changed",
                "CREATE TABLE `changed` (\n  `title` varchar(100) NOT NULL,\n  `id` bigint NOT NULL,\n  `new_col` enum('a','b,c') DEFAULT NULL,\n  PRIMARY KEY (`id`),\n  KEY `idx_title` (`title`)\n) ENGINE=InnoDB",
            )
            .expect("source changed table"),
        );

        let mut target = snapshot("target_db");
        target.tables.insert(
            "changed".to_owned(),
            parse_table_snapshot(
                "changed",
                "CREATE TABLE `changed` (\n  `id` int NOT NULL,\n  `old_col` varchar(10) DEFAULT NULL,\n  `title` varchar(20) DEFAULT NULL,\n  PRIMARY KEY (`id`),\n  KEY `idx_old` (`old_col`)\n) ENGINE=InnoDB",
            )
            .expect("target changed table"),
        );
        target.tables.insert(
            "removed".to_owned(),
            parse_table_snapshot(
                "removed",
                "CREATE TABLE `removed` (\n  `id` bigint NOT NULL\n) ENGINE=InnoDB",
            )
            .expect("removed table"),
        );

        let sql = build_schema_diff(&source, &target).expect("schema diff");
        assert!(sql.contains("CREATE TABLE `target_db`.`added`"));
        assert!(sql.contains("ALTER TABLE `target_db`.`changed` DROP INDEX `idx_old`;"));
        assert!(sql.contains("ALTER TABLE `target_db`.`changed` DROP COLUMN `old_col`;"));
        assert!(sql.contains("MODIFY COLUMN `title` varchar(100) NOT NULL FIRST;"));
        assert!(sql.contains("MODIFY COLUMN `id` bigint NOT NULL AFTER `title`;"));
        assert!(sql.contains("ADD COLUMN `new_col` enum('a','b,c') DEFAULT NULL AFTER `id`;"));
        assert!(sql.contains("ALTER TABLE `target_db`.`changed` ADD KEY `idx_title` (`title`);"));
        assert!(sql.contains("DROP TABLE `target_db`.`removed`;"));
    }

    #[test]
    fn foreign_keys_and_table_options_generate_ordered_migration_sql() {
        let mut source = snapshot("source_db");
        source.tables.insert(
            "relations".to_owned(),
            parse_table_snapshot(
                "relations",
                "CREATE TABLE `relations` (\n  `id` bigint NOT NULL AUTO_INCREMENT,\n  `parent_id` bigint NOT NULL,\n  PRIMARY KEY (`id`),\n  KEY `idx_parent` (`parent_id`),\n  CONSTRAINT `fk_parent` FOREIGN KEY (`parent_id`) REFERENCES `parents` (`id`) ON DELETE CASCADE\n) ENGINE=InnoDB AUTO_INCREMENT=42 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci COMMENT='source option'",
            )
            .expect("source relation table"),
        );

        let mut target = snapshot("target_db");
        target.tables.insert(
            "relations".to_owned(),
            parse_table_snapshot(
                "relations",
                "CREATE TABLE `relations` (\n  `id` bigint NOT NULL AUTO_INCREMENT,\n  `parent_id` bigint NOT NULL,\n  PRIMARY KEY (`id`),\n  KEY `idx_parent` (`parent_id`),\n  CONSTRAINT `fk_parent_old` FOREIGN KEY (`parent_id`) REFERENCES `parents` (`id`)\n) ENGINE=MyISAM AUTO_INCREMENT=7 DEFAULT CHARSET=latin1 COLLATE=latin1_swedish_ci COMMENT='target option'",
            )
            .expect("target relation table"),
        );

        let sql = build_schema_diff(&source, &target).expect("schema diff");
        let drop_position = sql
            .find("DROP FOREIGN KEY `fk_parent_old`")
            .expect("old foreign key drop");
        let options_position = sql
            .find("ENGINE=InnoDB, DEFAULT CHARACTER SET=utf8mb4, COLLATE=utf8mb4_unicode_ci, COMMENT='source option'")
            .expect("table option changes");
        let add_position = sql
            .find("ADD CONSTRAINT `fk_parent` FOREIGN KEY")
            .expect("new foreign key add");
        assert!(drop_position < options_position);
        assert!(options_position < add_position);
    }

    #[test]
    fn new_tables_create_before_their_foreign_keys_are_added() {
        let mut source = snapshot("source_db");
        source.tables.insert(
            "children".to_owned(),
            parse_table_snapshot(
                "children",
                "CREATE TABLE `children` (\n  `id` bigint NOT NULL,\n  `parent_id` bigint NOT NULL,\n  PRIMARY KEY (`id`),\n  CONSTRAINT `fk_child_parent` FOREIGN KEY (`parent_id`) REFERENCES `parents` (`id`)\n) ENGINE=InnoDB",
            )
            .expect("child table"),
        );

        let sql = build_schema_diff(&source, &snapshot("target_db")).expect("schema diff");
        let create_end = sql.find("ENGINE=InnoDB;").expect("table creation");
        let add_position = sql
            .find("ALTER TABLE `target_db`.`children` ADD CONSTRAINT `fk_child_parent`")
            .expect("foreign key add");
        assert!(create_end < add_position);
        assert!(!sql[..create_end].contains("CONSTRAINT `fk_child_parent`"));
    }

    #[test]
    fn referenced_table_changes_temporarily_rebuild_unchanged_foreign_keys() {
        let child_ddl = "CREATE TABLE `children` (\n  `id` bigint NOT NULL,\n  `parent_id` bigint NOT NULL,\n  PRIMARY KEY (`id`),\n  KEY `idx_parent` (`parent_id`),\n  CONSTRAINT `fk_child_parent` FOREIGN KEY (`parent_id`) REFERENCES `parents` (`id`)\n) ENGINE=InnoDB";
        let mut source = snapshot("source_db");
        source.tables.insert(
            "parents".to_owned(),
            parse_table_snapshot(
                "parents",
                "CREATE TABLE `parents` (\n  `id` bigint NOT NULL,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB",
            )
            .expect("source parent"),
        );
        source.tables.insert(
            "children".to_owned(),
            parse_table_snapshot("children", child_ddl).expect("source child"),
        );

        let mut target = snapshot("target_db");
        target.tables.insert(
            "parents".to_owned(),
            parse_table_snapshot(
                "parents",
                "CREATE TABLE `parents` (\n  `id` int NOT NULL,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB",
            )
            .expect("target parent"),
        );
        target.tables.insert(
            "children".to_owned(),
            parse_table_snapshot("children", child_ddl).expect("target child"),
        );

        let sql = build_schema_diff(&source, &target).expect("schema diff");
        let drop_position = sql
            .find("ALTER TABLE `target_db`.`children` DROP FOREIGN KEY `fk_child_parent`")
            .expect("foreign key drop");
        let parent_position = sql
            .find("ALTER TABLE `target_db`.`parents` MODIFY COLUMN `id` bigint NOT NULL FIRST")
            .expect("parent modification");
        let add_position = sql
            .find("ALTER TABLE `target_db`.`children` ADD CONSTRAINT `fk_child_parent`")
            .expect("foreign key restoration");
        assert!(drop_position < parent_position);
        assert!(parent_position < add_position);
    }

    #[test]
    fn views_are_dropped_created_replaced_and_retargeted() {
        let mut source = SchemaSnapshot {
            database_name: "source_db".to_owned(),
            ..SchemaSnapshot::default()
        };
        source.views.insert(
            "added_view".to_owned(),
            ViewSnapshot {
                definition: "select `source_db`.`items`.`id` AS `id` from `source_db`.`items`"
                    .to_owned(),
            },
        );
        source.views.insert(
            "changed_view".to_owned(),
            ViewSnapshot {
                definition:
                    "select `source_db`.`items`.`new_value` AS `new_value` from `source_db`.`items`"
                        .to_owned(),
            },
        );

        let mut target = SchemaSnapshot {
            database_name: "target_db".to_owned(),
            ..SchemaSnapshot::default()
        };
        target.views.insert(
            "changed_view".to_owned(),
            ViewSnapshot {
                definition:
                    "select `target_db`.`items`.`old_value` AS `old_value` from `target_db`.`items`"
                        .to_owned(),
            },
        );
        target.views.insert(
            "removed_view".to_owned(),
            ViewSnapshot {
                definition: "select 1 AS `value`".to_owned(),
            },
        );

        let sql = build_schema_diff(&source, &target).expect("view diff");
        assert!(sql.contains("DROP VIEW `target_db`.`removed_view`;"));
        assert!(sql.contains(
            "CREATE VIEW `target_db`.`added_view` AS select `target_db`.`items`.`id` AS `id` from `target_db`.`items`;"
        ));
        assert!(sql.contains(
            "CREATE OR REPLACE VIEW `target_db`.`changed_view` AS select `target_db`.`items`.`new_value` AS `new_value` from `target_db`.`items`;"
        ));
        assert!(!sql.contains("`source_db`."));
        assert_eq!(
            rewrite_qualified_catalog(
                "select '`source_db`.`items`' AS `literal`, `source_db`.`items`.`id` from `source_db`.`items`",
                "source_db",
                "target_db"
            ),
            "select '`source_db`.`items`' AS `literal`, `target_db`.`items`.`id` from `target_db`.`items`"
        );
    }

    #[test]
    fn schema_qualified_foreign_keys_are_retargeted_with_their_owner() {
        let mut source = snapshot("source_db");
        source.tables.insert(
            "children".to_owned(),
            parse_table_snapshot(
                "children",
                "CREATE TABLE `children` (\n  `id` bigint NOT NULL,\n  `parent_id` bigint NOT NULL,\n  PRIMARY KEY (`id`),\n  CONSTRAINT `fk_parent` FOREIGN KEY (`parent_id`) REFERENCES `source_db`.`parents` (`id`)\n) ENGINE=InnoDB",
            )
            .expect("qualified foreign key"),
        );

        let sql = build_schema_diff(&source, &snapshot("target_db")).expect("schema diff");
        assert!(sql.contains("ALTER TABLE `target_db`.`children` ADD CONSTRAINT `fk_parent`"));
        assert!(sql.contains("REFERENCES `target_db`.`parents` (`id`)"));
        assert!(!sql.contains("`source_db`."));
    }

    #[test]
    fn views_are_topologically_ordered_and_cycles_fail_closed() {
        let mut source = snapshot("source_db");
        source.views.insert(
            "a_child".to_owned(),
            ViewSnapshot {
                definition: "select * from `source_db`.`z_parent`".to_owned(),
            },
        );
        source.views.insert(
            "z_parent".to_owned(),
            ViewSnapshot {
                definition: "select 1 AS `value`".to_owned(),
            },
        );
        let sql = build_schema_diff(&source, &snapshot("target_db")).expect("ordered views");
        let parent = sql
            .find("CREATE VIEW `target_db`.`z_parent`")
            .expect("parent view");
        let child = sql
            .find("CREATE VIEW `target_db`.`a_child`")
            .expect("child view");
        assert!(parent < child);

        source.views.get_mut("z_parent").expect("parent").definition =
            "select * from `source_db`.`a_child`".to_owned();
        let error = build_schema_diff(&source, &snapshot("target_db"))
            .expect_err("view dependency cycle must fail closed");
        assert_eq!(
            error.api_error().code,
            "mysql_schema_diff_view_dependency_cycle"
        );
    }

    #[test]
    fn runtime_auto_increment_counter_is_not_schema_state() {
        let source_table = parse_table_snapshot(
            "items",
            "CREATE TABLE `items` (\n  `id` bigint NOT NULL AUTO_INCREMENT,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB AUTO_INCREMENT=42",
        )
        .expect("source table");
        assert!(
            !source_table
                .create_sql_without_foreign_keys
                .contains("AUTO_INCREMENT=42")
        );
        let target_table = parse_table_snapshot(
            "items",
            "CREATE TABLE `items` (\n  `id` bigint NOT NULL AUTO_INCREMENT,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB AUTO_INCREMENT=7",
        )
        .expect("target table");
        let mut source = snapshot("source_db");
        source.tables.insert("items".to_owned(), source_table);
        let mut target = snapshot("target_db");
        target.tables.insert("items".to_owned(), target_table);
        assert_eq!(
            build_schema_diff(&source, &target).expect("runtime counters are ignored"),
            NO_DIFFERENCES_SQL
        );
    }

    #[test]
    fn primary_key_replacement_uses_one_alter_statement() {
        let mut source = snapshot("source_db");
        source.tables.insert(
            "items".to_owned(),
            parse_table_snapshot(
                "items",
                "CREATE TABLE `items` (\n  `id` bigint NOT NULL AUTO_INCREMENT,\n  `code` bigint NOT NULL,\n  PRIMARY KEY (`code`),\n  KEY `idx_id` (`id`)\n) ENGINE=InnoDB",
            )
            .expect("source table"),
        );
        let mut target = snapshot("target_db");
        target.tables.insert(
            "items".to_owned(),
            parse_table_snapshot(
                "items",
                "CREATE TABLE `items` (\n  `id` bigint NOT NULL AUTO_INCREMENT,\n  `code` bigint NOT NULL,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB",
            )
            .expect("target table"),
        );

        let sql = build_schema_diff(&source, &target).expect("primary key diff");
        assert!(sql.contains(
            "ALTER TABLE `target_db`.`items` DROP PRIMARY KEY, ADD PRIMARY KEY (`code`);"
        ));
        assert!(!sql.contains("DROP PRIMARY KEY;"));
    }

    #[test]
    fn case_only_object_conflicts_fail_closed_on_case_insensitive_targets() {
        let table = || {
            parse_table_snapshot(
                "items",
                "CREATE TABLE `items` (\n  `id` bigint NOT NULL,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB",
            )
            .expect("table")
        };
        let mut source = snapshot("source_db");
        source.tables.insert("Items".to_owned(), table());
        let mut target = snapshot("target_db");
        target.lower_case_table_names = 1;
        target.tables.insert("items".to_owned(), table());

        let error = build_schema_diff(&source, &target)
            .expect_err("case-only conflict must not produce destructive SQL");
        assert_eq!(error.api_error().code, "mysql_schema_diff_case_conflict");
    }

    #[test]
    fn show_create_parser_keeps_generated_and_functional_index_commas_intact() {
        let table = parse_table_snapshot(
            "expressions",
            "CREATE TABLE `expressions` (\n  `id` bigint NOT NULL,\n  `label` varchar(20) GENERATED ALWAYS AS (concat('a,b',`id`)) STORED,\n  KEY `idx_expr` ((lower(`label`)))\n) ENGINE=InnoDB",
        )
        .expect("complex table ddl");

        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.indexes.len(), 1);
        assert!(table.columns[1].sql.contains("concat('a,b',`id`)"));
        assert!(table.indexes[0].sql.contains("(lower(`label`))"));
    }

    #[test]
    fn redundant_character_set_before_collation_has_one_stable_comparison_form() {
        assert_eq!(
            canonicalize_column_definition(
                "`label` varchar(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL"
            ),
            canonicalize_column_definition(
                "`label` varchar(64) COLLATE utf8mb4_unicode_ci NOT NULL"
            )
        );
        assert_ne!(
            canonicalize_column_definition(
                "`label` varchar(64) COLLATE utf8mb4_unicode_ci NOT NULL"
            ),
            canonicalize_column_definition(
                "`label` varchar(64) COLLATE utf8mb4_0900_ai_ci NOT NULL"
            )
        );
    }

    #[tokio::test]
    async fn invalid_request_fails_before_storage_or_java_access() {
        let error = Application::new()
            .preview_mysql_schema_diff(&CommunitySchemaDiffRequest {
                source: CommunitySchemaDiffEndpoint::default(),
                target: CommunitySchemaDiffEndpoint::default(),
            })
            .await
            .expect_err("missing source endpoint must fail");

        assert_eq!(
            error.api_error().code,
            "invalid_community_schema_diff_request"
        );
    }
}
