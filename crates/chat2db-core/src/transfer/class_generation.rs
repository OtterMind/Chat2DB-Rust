use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Seek, Write},
    path::{Path, PathBuf},
};

use chat2db_contract::{CommunityTableColumn, GenerateMysqlClassRequest, GeneratedMysqlClassSet};
use chat2db_storage::TransferArtifactRecord;
use uuid::Uuid;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{AppError, Application, native_mysql, now_millis};

const CLASS_ARCHIVE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
const MAX_CLASS_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;

struct RenderedClassSet {
    directory_name: String,
    files: Vec<(String, String)>,
}

pub(super) async fn generate(
    application: &Application,
    request: GenerateMysqlClassRequest,
) -> Result<GeneratedMysqlClassSet, AppError> {
    validate_desktop_request(&request)?;
    let rendered = render_request(application, &request).await?;

    tokio::task::spawn_blocking(move || write_class_set(&request.export_path, rendered))
        .await
        .map_err(|_| AppError::internal())?
}

pub(super) async fn generate_archive(
    application: &Application,
    request: GenerateMysqlClassRequest,
) -> Result<TransferArtifactRecord, AppError> {
    if !request.export_path.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_class_export_path",
            "Web class generation does not accept exportPath",
        ));
    }
    validate_table_name(&request.table_name)?;
    let rendered = render_request(application, &request).await?;
    let storage = application.require_storage()?;
    let file_name = format!(
        "{}-mybatis.zip",
        safe_archive_component(&request.table_name)
    );
    let expires_at_ms = now_millis()?.saturating_add(CLASS_ARCHIVE_TTL_MS);
    tokio::task::spawn_blocking(move || {
        let mut writer = storage
            .begin_transfer_artifact(
                None,
                &file_name,
                "application/zip",
                "ZIP",
                "zip",
                Some(expires_at_ms),
            )
            .map_err(AppError::from)?;
        write_class_archive(writer.file_mut(), &rendered)?;
        let byte_count = writer.file_mut().metadata().map_err(file_error)?.len();
        if byte_count > MAX_CLASS_ARCHIVE_BYTES {
            return Err(AppError::invalid(
                "class_archive_limit_exceeded",
                "The generated class archive is too large",
            ));
        }
        writer.finish().map_err(AppError::from)
    })
    .await
    .map_err(|_| AppError::internal())?
}

async fn render_request(
    application: &Application,
    request: &GenerateMysqlClassRequest,
) -> Result<RenderedClassSet, AppError> {
    let columns = native_mysql::list_columns(
        application,
        &request.datasource_id,
        &request.database_name,
        &request.schema_name,
        &request.table_name,
    )
    .await?
    .items;
    if columns.is_empty() {
        return Err(AppError::not_found(
            "mysql_table_not_found",
            "The selected MySQL table does not exist",
        ));
    }
    render_class_set(&request.table_name, &columns)
}

fn validate_desktop_request(request: &GenerateMysqlClassRequest) -> Result<(), AppError> {
    if request.export_path.trim().is_empty() || request.export_path.contains('\0') {
        return Err(AppError::invalid(
            "invalid_class_export_path",
            "exportPath must be a local directory",
        ));
    }
    validate_table_name(&request.table_name)
}

fn validate_table_name(table_name: &str) -> Result<(), AppError> {
    if table_name.trim().is_empty()
        || table_name.len() > 256
        || table_name.contains(['/', '\\', '\0'])
        || matches!(table_name, "." | "..")
    {
        return Err(AppError::invalid(
            "invalid_mysql_table_name",
            "tableName cannot be used as an output directory",
        ));
    }
    Ok(())
}

fn write_class_set(
    export_path: &str,
    rendered: RenderedClassSet,
) -> Result<GeneratedMysqlClassSet, AppError> {
    let base = PathBuf::from(export_path);
    fs::create_dir_all(&base).map_err(file_error)?;
    let base = fs::canonicalize(&base).map_err(file_error)?;
    let output = base.join(&rendered.directory_name);
    fs::create_dir_all(&output).map_err(file_error)?;
    let output = fs::canonicalize(&output).map_err(file_error)?;
    if !output.starts_with(&base) {
        return Err(AppError::invalid(
            "invalid_class_export_path",
            "The generated output directory escaped exportPath",
        ));
    }

    let mut written = Vec::with_capacity(rendered.files.len());
    for (name, contents) in rendered.files {
        let path = output.join(name);
        atomic_write(&path, contents.as_bytes())?;
        written.push(path.to_string_lossy().into_owned());
    }
    Ok(GeneratedMysqlClassSet {
        output_directory: output.to_string_lossy().into_owned(),
        files: written,
    })
}

fn render_class_set(
    table_name: &str,
    columns: &[CommunityTableColumn],
) -> Result<RenderedClassSet, AppError> {
    let class_name = format!("{}DO", upper_camel(table_name));
    let entity_name = format!("{class_name}.java");
    let mapper_name = format!("{}Mapper.java", upper_camel(table_name));
    let xml_name = format!("{}Mapper.xml", upper_camel(table_name));
    let files = vec![
        (entity_name, render_entity(&class_name, table_name, columns)),
        (mapper_name, render_mapper(&class_name, table_name)),
        (xml_name, render_mapper_xml(table_name)),
    ];
    let total_bytes = files.iter().try_fold(0_u64, |total, (_, contents)| {
        total.checked_add(u64::try_from(contents.len()).ok()?)
    });
    if total_bytes.is_none_or(|total| total > MAX_CLASS_ARCHIVE_BYTES) {
        return Err(AppError::invalid(
            "class_archive_limit_exceeded",
            "The generated class files are too large",
        ));
    }
    Ok(RenderedClassSet {
        directory_name: table_name.to_owned(),
        files,
    })
}

fn write_class_archive<W: Write + Seek>(
    output: W,
    rendered: &RenderedClassSet,
) -> Result<(), AppError> {
    let directory = safe_archive_component(&rendered.directory_name);
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, contents) in &rendered.files {
        zip.start_file(format!("{directory}/{name}"), options)
            .map_err(|error| zip_error(&error))?;
        zip.write_all(contents.as_bytes()).map_err(file_error)?;
    }
    zip.finish().map_err(|error| zip_error(&error))?;
    Ok(())
}

fn safe_archive_component(value: &str) -> String {
    let value: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if value.is_empty() {
        "generated".to_owned()
    } else {
        value
    }
}

fn render_entity(class_name: &str, table_name: &str, columns: &[CommunityTableColumn]) -> String {
    let mut imports = BTreeSet::from([
        "com.baomidou.mybatisplus.annotation.TableField",
        "com.baomidou.mybatisplus.annotation.TableName",
        "lombok.Data",
    ]);
    if columns
        .iter()
        .any(|column| column.primary_key == Some(true))
    {
        imports.insert("com.baomidou.mybatisplus.annotation.TableId");
    }
    for column in columns {
        match java_type(&column.column_type) {
            "BigDecimal" => {
                imports.insert("java.math.BigDecimal");
            }
            "LocalDate" => {
                imports.insert("java.time.LocalDate");
            }
            "LocalDateTime" => {
                imports.insert("java.time.LocalDateTime");
            }
            "LocalTime" => {
                imports.insert("java.time.LocalTime");
            }
            _ => {}
        }
    }

    let mut output = String::from("package com.my.entity;\n\n");
    for import in imports {
        output.push_str("import ");
        output.push_str(import);
        output.push_str(";\n");
    }
    output.push_str("\n@Data\n@TableName(\"");
    output.push_str(&java_string(table_name));
    output.push_str("\")\npublic class ");
    output.push_str(class_name);
    output.push_str(" {\n");

    let mut table_id_written = false;
    for column in columns {
        if column.comment.trim().is_empty() {
            output.push('\n');
        } else {
            output.push_str("\n    /** ");
            output.push_str(&javadoc(&column.comment));
            output.push_str(" */\n");
        }
        if column.primary_key == Some(true) && !table_id_written {
            output.push_str("    @TableId(\"");
            table_id_written = true;
        } else {
            output.push_str("    @TableField(\"");
        }
        output.push_str(&java_string(&column.name));
        output.push_str("\")\n    private ");
        output.push_str(java_type(&column.column_type));
        output.push(' ');
        output.push_str(&lower_camel(&column.name));
        output.push_str(";\n");
    }
    output.push_str("}\n");
    output
}

fn render_mapper(class_name: &str, table_name: &str) -> String {
    let mapper_name = format!("{}Mapper", upper_camel(table_name));
    format!(
        "package com.my.mapper;\n\n\
         import com.baomidou.mybatisplus.core.mapper.BaseMapper;\n\
         import com.my.entity.{class_name};\n\
         import org.apache.ibatis.annotations.Mapper;\n\n\
         @Mapper\n\
         public interface {mapper_name} extends BaseMapper<{class_name}> {{\n}}\n"
    )
}

fn render_mapper_xml(table_name: &str) -> String {
    let mapper_name = format!("{}Mapper", upper_camel(table_name));
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE mapper PUBLIC \"-//mybatis.org//DTD Mapper 3.0//EN\" \
         \"https://mybatis.org/dtd/mybatis-3-mapper.dtd\">\n\
         <mapper namespace=\"com.my.mapper.{mapper_name}\">\n\
         </mapper>\n"
    )
}

fn java_type(mysql_type: &str) -> &'static str {
    match mysql_type
        .split_ascii_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase()
        .as_str()
    {
        "BIGINT" => "Long",
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "INTEGER" | "YEAR" => "Integer",
        "DECIMAL" | "NUMERIC" => "BigDecimal",
        "FLOAT" => "Float",
        "DOUBLE" | "REAL" => "Double",
        "BIT" | "BOOL" | "BOOLEAN" => "Boolean",
        "DATE" => "LocalDate",
        "TIME" => "LocalTime",
        "DATETIME" | "TIMESTAMP" => "LocalDateTime",
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => "byte[]",
        _ => "String",
    }
}

fn upper_camel(value: &str) -> String {
    let mut output = String::new();
    let mut uppercase = true;
    for character in value.chars() {
        if character.is_alphanumeric() {
            if uppercase {
                output.extend(character.to_uppercase());
                uppercase = false;
            } else {
                output.push(character);
            }
        } else {
            uppercase = true;
        }
    }
    valid_java_identifier(output, "Generated")
}

fn lower_camel(value: &str) -> String {
    let upper = upper_camel(value);
    let mut characters = upper.chars();
    let Some(first) = characters.next() else {
        return "generated".to_owned();
    };
    let mut output = first.to_lowercase().collect::<String>();
    output.extend(characters);
    valid_java_identifier(output, "generated")
}

fn valid_java_identifier(mut value: String, fallback: &str) -> String {
    if value.is_empty() {
        value.push_str(fallback);
    }
    if value.starts_with(|character: char| character.is_ascii_digit()) {
        value.insert(0, '_');
    }
    if JAVA_KEYWORDS.contains(&value.as_str()) {
        value.push('_');
    }
    value
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(AppError::internal)?;
    let part = path.with_file_name(format!(".{file_name}.{}.part", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&part)
            .map_err(file_error)?;
        file.write_all(contents).map_err(file_error)?;
        file.sync_all().map_err(file_error)?;
        drop(file);
        fs::rename(&part, path).map_err(file_error)?;
        sync_parent(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(part);
    }
    result
}

fn sync_parent(path: &Path) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(AppError::internal)?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(file_error)
}

fn java_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn javadoc(value: &str) -> String {
    value
        .replace("*/", "* /")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_owned()
}

fn file_error(error: std::io::Error) -> AppError {
    tracing::warn!(%error, "MyBatis Plus class export filesystem operation failed");
    drop(error);
    AppError::unavailable(
        "class_export_failed",
        "The MyBatis Plus class files could not be written",
    )
}

fn zip_error(error: &zip::result::ZipError) -> AppError {
    tracing::warn!(%error, "MyBatis Plus class archive generation failed");
    AppError::unavailable(
        "class_archive_failed",
        "The MyBatis Plus class archive could not be generated",
    )
}

const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
];

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read as _};

    use super::{
        RenderedClassSet, java_type, lower_camel, safe_archive_component, upper_camel,
        write_class_archive,
    };

    #[test]
    fn java_names_and_types_are_stable() {
        assert_eq!(upper_camel("audit_log"), "AuditLog");
        assert_eq!(lower_camel("user-id"), "userId");
        assert_eq!(lower_camel("class"), "class_");
        assert_eq!(java_type("BIGINT UNSIGNED"), "Long");
        assert_eq!(java_type("TIMESTAMP"), "LocalDateTime");
    }

    #[test]
    fn archive_entries_are_relative_and_reuse_rendered_bytes() {
        let rendered = RenderedClassSet {
            directory_name: "audit log".to_owned(),
            files: vec![(
                "AuditLogDO.java".to_owned(),
                "class AuditLogDO {}\n".to_owned(),
            )],
        };
        let mut output = Cursor::new(Vec::new());
        write_class_archive(&mut output, &rendered).expect("archive writes");
        output.set_position(0);
        let mut archive = zip::ZipArchive::new(output).expect("archive opens");
        let mut entry = archive
            .by_name("audit_log/AuditLogDO.java")
            .expect("safe relative entry exists");
        let mut contents = String::new();
        entry.read_to_string(&mut contents).expect("entry reads");
        assert_eq!(contents, "class AuditLogDO {}\n");
        assert_eq!(safe_archive_component("../unsafe"), ".._unsafe");
    }
}
