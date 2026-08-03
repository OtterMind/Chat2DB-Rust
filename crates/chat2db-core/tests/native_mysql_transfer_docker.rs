use std::{
    fs::{self, File, OpenOptions},
    io::Read as _,
    panic::AssertUnwindSafe,
    path::Path,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    ComponentState, CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty,
    DmlExportFormat, DmlExportRequest, DmlExportSize, GenerateMysqlClassRequest, ImportFileRequest,
    OtherFileExportRequest, SqlFileExportRequest, TabularImportEncoding, TransferFileFormat,
    TransferSqlScope, TransferTask, TransferTaskStatus,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost, TransferArtifactDownload};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use futures_util::FutureExt as _;
use mysql_async::{Conn, Opts, OptsBuilder, prelude::Queryable};
use tempfile::TempDir;
use uuid::Uuid;
use xls::core::{Cell, Workbook};
use zip::ZipArchive;

const REQUIRED_MYSQL_ENV: [&str; 4] = [
    "MYSQL_TEST_HOST",
    "MYSQL_TEST_PORT",
    "MYSQL_TEST_USER",
    "MYSQL_TEST_PASSWORD",
];
const TASK_TIMEOUT: Duration = Duration::from_secs(30);
type TabularRoundTripRow = (
    u64,
    Option<String>,
    String,
    String,
    String,
    String,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
);

struct MysqlTestConfig {
    host: String,
    port: u16,
    user: String,
    password: String,
}

impl MysqlTestConfig {
    fn from_environment() -> Option<Self> {
        let required = mysql_test_required();
        let configured = REQUIRED_MYSQL_ENV
            .iter()
            .filter(|name| std::env::var_os(name).is_some())
            .count();
        if configured == 0 {
            assert!(
                !required,
                "MYSQL_TEST_REQUIRED is enabled but the MySQL endpoint is absent"
            );
            eprintln!("skipping native MySQL transfer test; MYSQL_TEST_* variables are absent");
            return None;
        }
        assert_eq!(
            configured,
            REQUIRED_MYSQL_ENV.len(),
            "native MySQL integration is partially configured"
        );
        let host = required_env("MYSQL_TEST_HOST");
        assert!(
            !host.trim().is_empty()
                && !host.chars().any(char::is_control)
                && !host.contains(['/', '?', '#']),
            "MYSQL_TEST_HOST is invalid"
        );
        let port = required_env("MYSQL_TEST_PORT")
            .parse::<u16>()
            .expect("MYSQL_TEST_PORT must be a TCP port");
        assert_ne!(port, 0, "MYSQL_TEST_PORT cannot be zero");
        let user = required_env("MYSQL_TEST_USER");
        assert!(!user.is_empty(), "MYSQL_TEST_USER cannot be empty");
        Some(Self {
            host,
            port,
            user,
            password: required_env("MYSQL_TEST_PASSWORD"),
        })
    }

    fn native_options(&self, database_name: Option<&str>) -> Opts {
        let mut builder = OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(Some(self.password.clone()))
            .prefer_socket(Some(false));
        if let Some(database_name) = database_name {
            builder = builder.db_name(Some(database_name.to_owned()));
        }
        builder.into()
    }

    fn connection(&self, database_name: &str) -> DatasourceConnection {
        let host = if self.host.contains(':')
            && !(self.host.starts_with('[') && self.host.ends_with(']'))
        {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:mysql://{host}:{}/{database_name}?useSSL=false&serverTimezone=UTC",
                self.port
            ),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: self.user.clone(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: self.password.clone(),
                    sensitive: true,
                },
            ],
            read_only: false,
            ssh: None,
        }
    }
}

#[tokio::test]
async fn native_mysql_transfer_product_keeps_java_dormant() {
    let Some(config) = MysqlTestConfig::from_environment() else {
        return;
    };
    let suffix = Uuid::new_v4().simple().to_string();
    let database_name = format!("chat2db_transfer_{}", &suffix[..12]);
    provision_database(&config, &database_name).await;

    let verification = AssertUnwindSafe(verify_transfer_product(&config, &database_name))
        .catch_unwind()
        .await;
    let cleanup = cleanup_database(&config, &database_name).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native MySQL transfer cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native MySQL transfer fixture must be removed");
}

#[allow(clippy::too_many_lines)]
async fn verify_transfer_product(config: &MysqlTestConfig, database_name: &str) {
    let directory = TempDir::new().expect("temporary native MySQL transfer runtime");
    let data_dir = directory.path().join("data");
    let missing_java = directory.path().join("missing-java");
    let mut host = RuntimeHost::open(runtime_config(&data_dir, &missing_java))
        .await
        .expect("native MySQL transfer runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native MySQL transfer".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(database_name)),
        })
        .await
        .expect("native MySQL transfer datasource must persist");

    verify_imports(
        &application,
        config,
        &datasource.id,
        database_name,
        directory.path(),
    )
    .await;
    assert_java_dormant(&application);

    let durable_export_task = verify_task_exports(
        &application,
        &datasource.id,
        database_name,
        directory.path(),
    )
    .await;
    verify_tabular_round_trip(&application, config, &datasource.id, database_name).await;
    assert_java_dormant(&application);

    verify_dml_exports_and_replay(&application, config, &datasource.id, database_name).await;
    verify_class_generation(
        &application,
        &datasource.id,
        database_name,
        directory.path(),
    )
    .await;
    verify_cancellation(
        &application,
        &datasource.id,
        database_name,
        directory.path(),
    )
    .await;
    assert_java_dormant(&application);

    let tasks = application
        .list_transfer_tasks(1, 100)
        .await
        .expect("transfer task history must list");
    assert!(tasks.total >= 14, "all product tasks must be retained");
    assert!(tasks.total <= 20, "task retention must remain bounded");

    drop(application);
    host.shutdown()
        .await
        .expect("native-only transfer runtime must shut down cleanly");
    drop(host);

    let mut reopened = RuntimeHost::open(runtime_config(&data_dir, &missing_java))
        .await
        .expect("native MySQL transfer runtime must reopen");
    let application = reopened.application();
    assert_java_dormant(&application);
    let task = application
        .transfer_task(durable_export_task)
        .await
        .expect("completed task must survive restart");
    assert_eq!(task.status, TransferTaskStatus::Succeeded);
    let download = application
        .transfer_task_artifact_download(durable_export_task)
        .await
        .expect("completed artifact must survive restart");
    assert!(download.path.is_file());
    drop(application);
    reopened
        .shutdown()
        .await
        .expect("reopened native runtime must shut down cleanly");
}

async fn verify_imports(
    application: &Application,
    config: &MysqlTestConfig,
    datasource_id: &str,
    database_name: &str,
    directory: &Path,
) {
    let csv_path = directory.join("import.csv");
    fs::write(
        &csv_path,
        "id,value_text\n1,csv-one\n2,csv-two\n3,__CHAT2DB_TRANSFER_V1__:NULL\n",
    )
    .expect("CSV fixture must write");
    import_and_succeed(
        application,
        ImportFileRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: Some("import_csv".to_owned()),
            file_path: csv_path.to_string_lossy().into_owned(),
            format: TransferFileFormat::Csv,
            contains_header: true,
            tabular_encoding: TabularImportEncoding::Plain,
        },
    )
    .await;

    for (format, table_name, file_name, value) in [
        (
            TransferFileFormat::Xls,
            "import_xls",
            "import.xls",
            "xls-one",
        ),
        (
            TransferFileFormat::Xlsx,
            "import_xlsx",
            "import.xlsx",
            "xlsx-one",
        ),
    ] {
        let path = directory.join(file_name);
        write_spreadsheet(&path, format, value);
        import_and_succeed(
            application,
            ImportFileRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                schema_name: String::new(),
                table_name: Some(table_name.to_owned()),
                file_path: path.to_string_lossy().into_owned(),
                format,
                contains_header: true,
                tabular_encoding: TabularImportEncoding::Plain,
            },
        )
        .await;
    }

    let sql_path = directory.join("import.sql");
    fs::write(
        &sql_path,
        "CREATE TABLE sql_loaded (id BIGINT PRIMARY KEY, value_text VARCHAR(64));\n\
         INSERT INTO sql_loaded VALUES (1, 'selected-database');\n",
    )
    .expect("SQL fixture must write");
    import_and_succeed(
        application,
        ImportFileRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: None,
            file_path: sql_path.to_string_lossy().into_owned(),
            format: TransferFileFormat::Sql,
            contains_header: false,
            tabular_encoding: TabularImportEncoding::Plain,
        },
    )
    .await;

    let mut conn = Conn::new(config.native_options(Some(database_name)))
        .await
        .expect("import verification connection must open");
    for (table_name, expected) in [
        (
            "import_csv",
            vec!["csv-one", "csv-two", "__CHAT2DB_TRANSFER_V1__:NULL"],
        ),
        ("import_xls", vec!["xls-one"]),
        ("import_xlsx", vec!["xlsx-one"]),
        ("sql_loaded", vec!["selected-database"]),
    ] {
        let values: Vec<String> = conn
            .query(format!("SELECT value_text FROM `{table_name}` ORDER BY id"))
            .await
            .expect("imported rows must query");
        assert_eq!(values, expected);
    }
    conn.disconnect()
        .await
        .expect("import verification connection must close");
}

async fn verify_sql_task_exports(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    user_export_path: &Path,
) -> i64 {
    let mut durable_task_id = 0_i64;
    for scope in [
        TransferSqlScope::All,
        TransferSqlScope::Schema,
        TransferSqlScope::Table,
    ] {
        let task = application
            .export_mysql_sql_file(SqlFileExportRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                schema_name: String::new(),
                table_names: vec!["source_a".to_owned()],
                scope,
                export_path: (scope == TransferSqlScope::All)
                    .then(|| user_export_path.to_string_lossy().into_owned()),
            })
            .await
            .expect("SQL export task must start");
        let completed = wait_for_terminal_task(application, task.task_id).await;
        assert_task_succeeded(&completed);
        let download = application
            .transfer_task_artifact_download(task.task_id)
            .await
            .expect("SQL task artifact must download by task id");
        let sql = fs::read_to_string(&download.path).expect("SQL artifact must be UTF-8");
        match scope {
            TransferSqlScope::All => {
                assert!(sql.contains("CREATE TABLE"));
                assert!(sql.contains("INSERT INTO"));
                assert!(
                    user_export_path
                        .join(&download.artifact.file_name)
                        .is_file()
                );
                durable_task_id = task.task_id;
            }
            TransferSqlScope::Schema => {
                assert!(sql.contains("CREATE TABLE"));
                assert!(!sql.contains("INSERT INTO"));
            }
            TransferSqlScope::Table => {
                assert!(!sql.contains("CREATE TABLE"));
                assert!(sql.contains("INSERT INTO"));
            }
        }
    }
    durable_task_id
}

async fn verify_task_exports(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    directory: &Path,
) -> i64 {
    let user_export_path = directory.join("user-exports");
    let durable_task_id =
        verify_sql_task_exports(application, datasource_id, database_name, &user_export_path).await;

    for format in [
        TransferFileFormat::Csv,
        TransferFileFormat::Xls,
        TransferFileFormat::Xlsx,
    ] {
        let download = export_other_and_download(
            application,
            OtherFileExportRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                schema_name: String::new(),
                table_names: vec!["source_a".to_owned()],
                format,
                contains_header: true,
                export_path: None,
            },
        )
        .await;
        assert_single_tabular_export(&download, format);
    }

    let csv_zip = export_other_and_download(
        application,
        OtherFileExportRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_names: vec!["source_a".to_owned(), "source_b".to_owned()],
            format: TransferFileFormat::Csv,
            contains_header: true,
            export_path: None,
        },
    )
    .await;
    assert_zip_entries(&csv_zip.path, &["source_a.csv", "source_b.csv"], "id");

    let sql_zip = export_other_and_download(
        application,
        OtherFileExportRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_names: vec!["source_a".to_owned(), "source_b".to_owned()],
            format: TransferFileFormat::Sql,
            contains_header: true,
            export_path: None,
        },
    )
    .await;
    assert_eq!(sql_zip.artifact.format, "ZIP");
    assert_zip_entries(
        &sql_zip.path,
        &["source_a.sql", "source_b.sql"],
        "INSERT INTO",
    );
    durable_task_id
}

async fn verify_tabular_round_trip(
    application: &Application,
    config: &MysqlTestConfig,
    datasource_id: &str,
    database_name: &str,
) {
    for (format, target_table) in [
        (TransferFileFormat::Csv, "roundtrip_csv"),
        (TransferFileFormat::Xls, "roundtrip_xls"),
        (TransferFileFormat::Xlsx, "roundtrip_xlsx"),
    ] {
        let download = export_other_and_download(
            application,
            OtherFileExportRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                schema_name: String::new(),
                table_names: vec!["source_roundtrip".to_owned()],
                format,
                contains_header: true,
                export_path: None,
            },
        )
        .await;
        assert_round_trip_export_is_readable(&download.path, format);
        import_and_succeed(
            application,
            ImportFileRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                schema_name: String::new(),
                table_name: Some(target_table.to_owned()),
                file_path: download.path.to_string_lossy().into_owned(),
                format,
                contains_header: true,
                tabular_encoding: TabularImportEncoding::Chat2dbV1,
            },
        )
        .await;
    }

    let mut conn = Conn::new(config.native_options(Some(database_name)))
        .await
        .expect("tabular round-trip verification connection must open");
    for table_name in ["roundtrip_csv", "roundtrip_xls", "roundtrip_xlsx"] {
        let row: Option<TabularRoundTripRow> = conn
            .exec_first(
                format!(
                    "SELECT id, nullable_text, empty_text, utf8_text, decimal_value, \
                     CAST(timestamp_value AS CHAR), bit_value, payload, blob_value \
                     FROM `{table_name}`"
                ),
                (),
            )
            .await
            .expect("round-tripped row must query");
        let row = row.expect("round-tripped row must exist");
        assert_eq!(row.0, 1);
        assert_eq!(row.1, None, "{table_name} must preserve NULL");
        assert_eq!(row.2, "", "{table_name} must preserve empty text");
        assert_eq!(
            row.3, "utf8-\u{4e2d}\u{6587}",
            "{table_name} must preserve UTF-8 text"
        );
        assert_eq!(
            row.4, "1234567890.123400",
            "{table_name} must preserve readable DECIMAL"
        );
        assert_eq!(
            row.5, "2024-02-03 04:05:06.123456",
            "{table_name} must preserve readable TIMESTAMP"
        );
        assert_eq!(
            row.6,
            vec![0x01, 0x01],
            "{table_name} must preserve BIT bytes"
        );
        assert_eq!(
            row.7,
            vec![0x00, 0xff],
            "{table_name} must preserve VARBINARY bytes"
        );
        assert_eq!(
            row.8,
            vec![0x00, 0xff, b'B'],
            "{table_name} must preserve BLOB bytes"
        );
    }
    conn.disconnect()
        .await
        .expect("tabular round-trip verification connection must close");
}

fn assert_round_trip_export_is_readable(path: &Path, format: TransferFileFormat) {
    match format {
        TransferFileFormat::Csv => {
            let mut reader =
                csv::Reader::from_path(path).expect("round-trip CSV artifact must decode");
            let record = reader
                .records()
                .next()
                .expect("round-trip CSV row must exist")
                .expect("round-trip CSV row must decode");
            assert_eq!(&record[0], "1", "ordinary numeric values stay readable");
            assert_eq!(&record[1], "__CHAT2DB_TRANSFER_V1__:NULL");
            assert_eq!(&record[2], "", "empty text stays an empty CSV field");
            assert_eq!(&record[3], "utf8-\u{4e2d}\u{6587}");
            assert_eq!(&record[4], "1234567890.123400");
            assert_eq!(&record[5], "2024-02-03 04:05:06.123456");
            for index in [6, 7, 8] {
                assert!(record[index].starts_with("__CHAT2DB_TRANSFER_V1__:BASE64:"));
            }
        }
        TransferFileFormat::Xls | TransferFileFormat::Xlsx => {
            let file = File::open(path).expect("round-trip spreadsheet must open");
            let workbook = match format {
                TransferFileFormat::Xls => xls::core::xls::read(file),
                TransferFileFormat::Xlsx => xls::core::xlsx::read(file),
                TransferFileFormat::Csv | TransferFileFormat::Sql => unreachable!(),
            }
            .expect("round-trip spreadsheet must decode");
            assert_eq!(workbook.display_cell(0, 1, 0), "1");
            assert_eq!(workbook.display_cell(0, 1, 2), "");
            assert_eq!(workbook.display_cell(0, 1, 3), "utf8-\u{4e2d}\u{6587}");
            assert_eq!(workbook.display_cell(0, 1, 4), "1234567890.123400");
            assert_eq!(workbook.display_cell(0, 1, 5), "2024-02-03 04:05:06.123456");
            for column in [6, 7, 8] {
                assert!(
                    workbook
                        .display_cell(0, 1, column)
                        .starts_with("__CHAT2DB_TRANSFER_V1__:BASE64:")
                );
            }
        }
        TransferFileFormat::Sql => panic!("SQL is not a tabular round-trip format"),
    }
}

async fn verify_current_page_dml_csv(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    original_sql: &str,
) {
    let csv = application
        .export_mysql_dml(DmlExportRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            sql: format!("{original_sql} LIMIT 1"),
            original_sql: original_sql.to_owned(),
            result_set_id: Some(0),
            export_size: DmlExportSize::CurrentPage,
            format: DmlExportFormat::Csv,
        })
        .await
        .expect("current-page DML CSV must export");
    let csv = application
        .transfer_artifact_download(&csv.id)
        .await
        .expect("DML CSV artifact must resolve");
    let mut csv_reader = csv::Reader::from_path(csv.path).expect("DML CSV must decode");
    assert_eq!(
        csv_reader.records().count(),
        1,
        "current-page export uses sql"
    );
}

async fn verify_dml_exports_and_replay(
    application: &Application,
    config: &MysqlTestConfig,
    datasource_id: &str,
    database_name: &str,
) {
    let original_sql =
        format!("SELECT id, note, payload FROM `{database_name}`.`source_a` ORDER BY id");
    verify_current_page_dml_csv(application, datasource_id, database_name, &original_sql).await;

    let xlsx = application
        .export_mysql_dml(DmlExportRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            sql: String::new(),
            original_sql: original_sql.clone(),
            result_set_id: Some(0),
            export_size: DmlExportSize::All,
            format: DmlExportFormat::Xlsx,
        })
        .await
        .expect("all-row DML XLSX must export");
    let xlsx = application
        .transfer_artifact_download(&xlsx.id)
        .await
        .expect("DML XLSX artifact must resolve");
    let workbook = xls::core::xlsx::read(File::open(xlsx.path).expect("XLSX must open"))
        .expect("DML XLSX must decode");
    assert_eq!(workbook.sheets[0].dimensions().0, 3);

    let inserts = application
        .export_mysql_dml(DmlExportRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            sql: String::new(),
            original_sql,
            result_set_id: Some(0),
            export_size: DmlExportSize::All,
            format: DmlExportFormat::Insert,
        })
        .await
        .expect("DML INSERT must export");
    let inserts = application
        .transfer_artifact_download(&inserts.id)
        .await
        .expect("DML INSERT artifact must resolve");
    let insert_sql = fs::read_to_string(&inserts.path).expect("INSERT export must be UTF-8");
    assert_eq!(insert_sql.matches("INSERT INTO").count(), 2);

    let mut conn = Conn::new(config.native_options(Some(database_name)))
        .await
        .expect("DML replay verification connection must open");
    conn.query_drop("TRUNCATE TABLE source_a")
        .await
        .expect("DML replay table must clear");
    conn.disconnect()
        .await
        .expect("DML replay preparation connection must close");
    import_and_succeed(
        application,
        ImportFileRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: None,
            file_path: inserts.path.to_string_lossy().into_owned(),
            format: TransferFileFormat::Sql,
            contains_header: false,
            tabular_encoding: TabularImportEncoding::Plain,
        },
    )
    .await;

    let mut conn = Conn::new(config.native_options(Some(database_name)))
        .await
        .expect("DML replay result connection must open");
    let rows: Vec<(String, Vec<u8>)> = conn
        .query("SELECT note, payload FROM source_a ORDER BY id")
        .await
        .expect("replayed INSERT rows must query");
    assert_eq!(rows[0].0, "quote ' slash \\ newline\nnext");
    assert_eq!(rows[0].1, vec![0x00, 0x01, 0xff]);
    assert_eq!(rows[1].0, "plain");
    conn.disconnect()
        .await
        .expect("DML replay result connection must close");
}

async fn verify_class_generation(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    directory: &Path,
) {
    let output = directory.join("generated");
    let generated = application
        .generate_mysql_classes(GenerateMysqlClassRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: "source_a".to_owned(),
            export_path: output.to_string_lossy().into_owned(),
        })
        .await
        .expect("MyBatis Plus classes must generate");
    assert_eq!(generated.files.len(), 3);
    let contents = generated
        .files
        .iter()
        .map(|path| fs::read_to_string(path).expect("generated file must read"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(contents.contains("@TableName(\"source_a\")"));
    assert!(contents.contains("SourceAMapper"));
    assert!(contents.contains("<mapper namespace="));
}

async fn verify_cancellation(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    directory: &Path,
) {
    let path = directory.join("cancel.sql");
    fs::write(&path, "SELECT SLEEP(30);\n").expect("cancellation SQL must write");
    let accepted = application
        .import_mysql_file(ImportFileRequest {
            datasource_id: datasource_id.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: None,
            file_path: path.to_string_lossy().into_owned(),
            format: TransferFileFormat::Sql,
            contains_header: false,
            tabular_encoding: TabularImportEncoding::Plain,
        })
        .await
        .expect("cancellable import must start");
    wait_for_running_task(application, accepted.task_id).await;
    application
        .stop_transfer_task(accepted.task_id)
        .await
        .expect("running transfer must accept cancellation");
    let task = wait_for_terminal_task(application, accepted.task_id).await;
    assert_eq!(task.status, TransferTaskStatus::Cancelled);
    assert!(task.cancel_requested);
}

async fn import_and_succeed(application: &Application, request: ImportFileRequest) {
    let accepted = application
        .import_mysql_file(request)
        .await
        .expect("import task must start");
    let task = wait_for_terminal_task(application, accepted.task_id).await;
    assert_task_succeeded(&task);
}

async fn export_other_and_download(
    application: &Application,
    request: OtherFileExportRequest,
) -> TransferArtifactDownload {
    let accepted = application
        .export_mysql_other_file(request)
        .await
        .expect("other-file export task must start");
    let task = wait_for_terminal_task(application, accepted.task_id).await;
    assert_task_succeeded(&task);
    application
        .transfer_task_artifact_download(accepted.task_id)
        .await
        .expect("other-file artifact must download by task id")
}

async fn wait_for_running_task(application: &Application, task_id: i64) {
    tokio::time::timeout(TASK_TIMEOUT, async {
        loop {
            let task = application
                .transfer_task(task_id)
                .await
                .expect("transfer task must remain readable");
            match task.status {
                TransferTaskStatus::Running => return,
                TransferTaskStatus::Queued => tokio::time::sleep(Duration::from_millis(20)).await,
                status => panic!("task became {status:?} before cancellation"),
            }
        }
    })
    .await
    .expect("transfer task must enter running state");
}

async fn wait_for_terminal_task(application: &Application, task_id: i64) -> TransferTask {
    tokio::time::timeout(TASK_TIMEOUT, async {
        loop {
            let task = application
                .transfer_task(task_id)
                .await
                .expect("transfer task must remain readable");
            if matches!(
                task.status,
                TransferTaskStatus::Succeeded
                    | TransferTaskStatus::Failed
                    | TransferTaskStatus::Cancelled
                    | TransferTaskStatus::Interrupted
            ) {
                return task;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("transfer task must finish before timeout")
}

fn assert_task_succeeded(task: &TransferTask) {
    assert_eq!(
        task.status,
        TransferTaskStatus::Succeeded,
        "transfer task failed: {}",
        task.error_log
    );
}

fn assert_single_tabular_export(download: &TransferArtifactDownload, format: TransferFileFormat) {
    match format {
        TransferFileFormat::Csv => {
            let value = fs::read_to_string(&download.path).expect("CSV export must decode");
            assert!(value.starts_with("id,note,payload"));
        }
        TransferFileFormat::Xls => {
            let bytes = fs::read(&download.path).expect("XLS export must read");
            assert_eq!(&bytes[..4], &[0xd0, 0xcf, 0x11, 0xe0]);
            let workbook = xls::core::xls::read(File::open(&download.path).expect("XLS opens"))
                .expect("XLS export must decode");
            assert_eq!(workbook.display_cell(0, 0, 0), "id");
        }
        TransferFileFormat::Xlsx => {
            let bytes = fs::read(&download.path).expect("XLSX export must read");
            assert_eq!(&bytes[..4], b"PK\x03\x04");
            let workbook = xls::core::xlsx::read(File::open(&download.path).expect("XLSX opens"))
                .expect("XLSX export must decode");
            assert_eq!(workbook.display_cell(0, 0, 0), "id");
        }
        TransferFileFormat::Sql => panic!("SQL is not a tabular export in this assertion"),
    }
}

fn assert_zip_entries(path: &Path, expected_names: &[&str], expected_text: &str) {
    let bytes = fs::read(path).expect("ZIP export must read");
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    let mut archive = ZipArchive::new(File::open(path).expect("ZIP export must open"))
        .expect("ZIP export must decode");
    assert_eq!(archive.len(), expected_names.len());
    for expected_name in expected_names {
        let mut entry = archive
            .by_name(expected_name)
            .unwrap_or_else(|_| panic!("ZIP entry {expected_name} must exist"));
        let mut contents = String::new();
        entry
            .read_to_string(&mut contents)
            .expect("ZIP text entry must decode");
        assert!(contents.contains(expected_text));
    }
}

fn write_spreadsheet(path: &Path, format: TransferFileFormat, value: &str) {
    let mut workbook = Workbook::new();
    let sheet = workbook.sheet_mut(0).expect("default worksheet must exist");
    for (row, values) in [["id", "value_text"], ["1", value]].into_iter().enumerate() {
        for (column, value) in values.into_iter().enumerate() {
            sheet.set(
                u32::try_from(row).expect("fixture row fits"),
                u32::try_from(column).expect("fixture column fits"),
                Cell::Text(value.to_owned()),
            );
        }
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)
        .expect("spreadsheet fixture must create");
    match format {
        TransferFileFormat::Xls => xls::core::xls::write(&workbook, &mut file),
        TransferFileFormat::Xlsx => xls::core::xlsx::write(&workbook, &mut file),
        TransferFileFormat::Csv | TransferFileFormat::Sql => unreachable!(),
    }
    .expect("spreadsheet fixture must encode");
}

fn runtime_config(data_dir: &Path, missing_java: &Path) -> RuntimeConfig {
    RuntimeConfig::new(EngineConfig::new(EngineCommand::new(
        missing_java.to_owned(),
    )))
    .with_data_dir(data_dir)
    .with_vault_master_key_base64(STANDARD.encode([0x74; 32]))
}

async fn provision_database(config: &MysqlTestConfig, database_name: &str) {
    let mut conn = Conn::new(config.native_options(None))
        .await
        .expect("native MySQL transfer fixture connection must open");
    conn.query_drop(format!(
        "CREATE DATABASE `{database_name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci"
    ))
    .await
    .expect("native MySQL transfer fixture database must create");
    for table_name in ["import_csv", "import_xls", "import_xlsx"] {
        conn.query_drop(format!(
            "CREATE TABLE `{database_name}`.`{table_name}` (\
             id BIGINT PRIMARY KEY, value_text VARCHAR(64) NOT NULL) ENGINE=InnoDB"
        ))
        .await
        .expect("native MySQL import target must create");
    }
    conn.query_drop(format!(
        "CREATE TABLE `{database_name}`.`source_a` (\
         id BIGINT PRIMARY KEY, note VARCHAR(255) NOT NULL, payload VARBINARY(255) NOT NULL\
         ) ENGINE=InnoDB"
    ))
    .await
    .expect("native MySQL source A must create");
    conn.exec_drop(
        format!("INSERT INTO `{database_name}`.`source_a` (id, note, payload) VALUES (?, ?, ?)"),
        (
            1_u64,
            "quote ' slash \\ newline\nnext",
            vec![0x00, 0x01, 0xff],
        ),
    )
    .await
    .expect("native MySQL special source row must insert");
    conn.exec_drop(
        format!("INSERT INTO `{database_name}`.`source_a` (id, note, payload) VALUES (?, ?, ?)"),
        (2_u64, "plain", vec![0x02, 0x03]),
    )
    .await
    .expect("native MySQL plain source row must insert");
    conn.query_drop(format!(
        "CREATE TABLE `{database_name}`.`source_b` (\
         id BIGINT PRIMARY KEY, label VARCHAR(64) NOT NULL\
         ) ENGINE=InnoDB"
    ))
    .await
    .expect("native MySQL source B must create");
    conn.query_drop(format!(
        "INSERT INTO `{database_name}`.`source_b` VALUES (1, 'second-table')"
    ))
    .await
    .expect("native MySQL source B row must insert");
    for table_name in [
        "source_roundtrip",
        "roundtrip_csv",
        "roundtrip_xls",
        "roundtrip_xlsx",
    ] {
        conn.query_drop(format!(
            "CREATE TABLE `{database_name}`.`{table_name}` (\
             id BIGINT PRIMARY KEY, nullable_text VARCHAR(64) NULL, \
             empty_text VARCHAR(64) NOT NULL, utf8_text VARCHAR(64) NOT NULL, \
             decimal_value DECIMAL(20, 6) NOT NULL, timestamp_value TIMESTAMP(6) NOT NULL, \
             bit_value BIT(9) NOT NULL, payload VARBINARY(64) NOT NULL, \
             blob_value BLOB NOT NULL\
             ) ENGINE=InnoDB"
        ))
        .await
        .expect("native MySQL tabular round-trip table must create");
    }
    conn.exec_drop(
        format!(
            "INSERT INTO `{database_name}`.`source_roundtrip` \
             (id, nullable_text, empty_text, utf8_text, decimal_value, timestamp_value, \
              bit_value, payload, blob_value) \
             VALUES (?, ?, ?, ?, ?, ?, b'100000001', ?, ?)"
        ),
        (
            1_u64,
            Option::<String>::None,
            String::new(),
            "utf8-\u{4e2d}\u{6587}",
            "1234567890.123400",
            "2024-02-03 04:05:06.123456",
            vec![0x00, 0xff],
            vec![0x00, 0xff, b'B'],
        ),
    )
    .await
    .expect("native MySQL tabular round-trip source row must insert");
    conn.disconnect()
        .await
        .expect("native MySQL transfer fixture connection must close");
}

async fn cleanup_database(config: &MysqlTestConfig, database_name: &str) -> Result<(), String> {
    let mut conn = Conn::new(config.native_options(None))
        .await
        .map_err(|error| error.to_string())?;
    conn.query_drop(format!("DROP DATABASE IF EXISTS `{database_name}`"))
        .await
        .map_err(|error| error.to_string())?;
    conn.disconnect().await.map_err(|error| error.to_string())
}

fn assert_java_dormant(application: &Application) {
    let engine = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "database-engine")
        .expect("database engine health must be present");
    assert_eq!(engine.state, ComponentState::Ready);
    assert_eq!(engine.detail, "Available on demand; Java is not running");
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be configured"))
}

fn mysql_test_required() -> bool {
    match std::env::var("MYSQL_TEST_REQUIRED") {
        Err(std::env::VarError::NotPresent) => false,
        Ok(value) if value == "1" || value.eq_ignore_ascii_case("true") => true,
        Ok(value) if value == "0" || value.eq_ignore_ascii_case("false") => false,
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => {
            panic!("MYSQL_TEST_REQUIRED must be 1, 0, true, or false")
        }
    }
}
