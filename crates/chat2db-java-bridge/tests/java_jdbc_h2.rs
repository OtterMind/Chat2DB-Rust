use std::{fmt::Write as _, path::PathBuf, time::Duration};

use chat2db_java_bridge::{
    BridgeError, CancelDisposition, ColumnNullability, DriverArtifact, DriverClient, DriverSpec,
    EngineCommand, EngineConfig, EngineSupervisor, JdbcParameter, JdbcRow, JdbcValue,
    JdbcValueType, LoadedDriver, QueryEvent, QueryOptions, QueryRequest, Session, SessionConfig,
    SessionState, Transaction, TransactionOptions, UpdateRequest,
};
use sha2::{Digest, Sha256};

const H2_DRIVER_CLASS: &str = "org.h2.Driver";
const NO_EVENT_WINDOW: Duration = Duration::from_millis(250);
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESULT_BYTES: u64 = 1024 * 1024;

struct H2Harness {
    supervisor: EngineSupervisor,
    driver_client: DriverClient,
    loaded_driver: LoadedDriver,
    jdbc_url: String,
}

impl H2Harness {
    async fn start(database_name: &str) -> Self {
        let engine_jar = required_jar("CHAT2DB_JAVA_ENGINE_JAR");
        let h2_jar = required_jar("CHAT2DB_H2_DRIVER_JAR");
        let supervisor = EngineSupervisor::spawn(
            EngineConfig::new(EngineCommand::java_jar("java", engine_jar)).with_timeouts(
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(5),
            ),
        )
        .await
        .expect("packaged Java engine must handshake");
        let driver_client = supervisor
            .client()
            .driver_client()
            .expect("ready Java engine must create a driver client");
        let artifact =
            DriverArtifact::from_path(h2_jar).expect("H2 JAR must be a valid driver artifact");
        let expected_driver_id =
            expected_driver_id(H2_DRIVER_CLASS, std::slice::from_ref(&artifact));
        let loaded_driver = driver_client
            .load_driver(DriverSpec {
                driver_class: H2_DRIVER_CLASS.to_owned(),
                artifacts: vec![artifact],
            })
            .await
            .expect("Java engine must load the external H2 driver");

        assert_eq!(loaded_driver.driver_id, expected_driver_id);
        assert_eq!(loaded_driver.driver_class, H2_DRIVER_CLASS);
        assert_eq!(loaded_driver.artifact_count, 1);
        assert!(loaded_driver.driver_id.starts_with("sha256:"));
        assert_eq!(loaded_driver.driver_id.len(), "sha256:".len() + 64);

        Self {
            supervisor,
            driver_client,
            loaded_driver,
            jdbc_url: format!(
                "jdbc:h2:mem:{database_name};DB_CLOSE_DELAY=-1;DATABASE_TO_UPPER=TRUE"
            ),
        }
    }

    async fn open_session(&self) -> Session {
        self.driver_client
            .open_session(SessionConfig {
                driver_id: self.loaded_driver.driver_id.clone(),
                jdbc_url: self.jdbc_url.clone(),
                properties: Vec::new(),
                read_only: false,
            })
            .await
            .expect("H2 session must open")
    }

    async fn finish(self) {
        self.driver_client
            .unload_driver(self.loaded_driver.driver_id)
            .await
            .expect("closed H2 driver must unload");
        let exit = self
            .supervisor
            .shutdown()
            .await
            .expect("Java engine must shut down");
        assert!(exit.success);
    }
}

#[tokio::test]
async fn loads_h2_and_streams_typed_rows_only_when_credited() {
    let harness = H2Harness::start("typed_stream").await;
    let session = harness.open_session().await;

    assert_eq!(session.database().name, "H2");
    assert!(session.database().version.contains("2.3.232"));
    assert_eq!(session.database().driver_name, "H2 JDBC Driver");
    assert!(session.database().driver_version.contains("2.3.232"));
    assert_eq!(session.state().await, SessionState::AutoCommit);

    let mut stream = session
        .execute_query(QueryRequest {
            sql: "SELECT X AS id, CAST('row-' || X AS VARCHAR(16)) AS label, \
                  MOD(X, 2) = 0 AS active, \
                  CAST(X + 0.25 AS DECIMAL(10, 2)) AS amount \
                  FROM SYSTEM_RANGE(1, 5) ORDER BY X"
                .to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
            options: QueryOptions {
                max_rows: 0,
                target_batch_rows: 2,
                target_batch_bytes: 1024,
                initial_batch_credits: 0,
                max_result_bytes: MAX_RESULT_BYTES,
            },
        })
        .await
        .expect("typed H2 query must start");

    let QueryEvent::Started(started) = next_event(&mut stream).await else {
        panic!("the first query event must contain metadata");
    };
    assert_eq!(started.columns.len(), 4);
    assert_eq!(
        started
            .columns
            .iter()
            .map(|column| column.label.as_str())
            .collect::<Vec<_>>(),
        ["ID", "LABEL", "ACTIVE", "AMOUNT"]
    );
    assert_eq!(
        started
            .columns
            .iter()
            .map(|column| column.value_type)
            .collect::<Vec<_>>(),
        [
            JdbcValueType::SignedInteger,
            JdbcValueType::Text,
            JdbcValueType::Boolean,
            JdbcValueType::Decimal,
        ]
    );
    assert_eq!(started.columns[0].ordinal, 1);
    assert_eq!(started.columns[0].nullability, ColumnNullability::Nullable);

    assert_no_event(&mut stream).await;

    let mut rows = Vec::new();
    for (expected_offset, expected_len) in [(0, 2), (2, 2), (4, 1)] {
        assert_eq!(
            stream
                .grant_credits(1)
                .await
                .expect("one batch credit must be accepted"),
            1
        );
        let QueryEvent::Batch(batch) = next_event(&mut stream).await else {
            panic!("a credit must release exactly one row batch");
        };
        assert_eq!(batch.start_row_offset, expected_offset);
        assert_eq!(batch.rows.len(), expected_len);
        rows.extend(batch.rows);
        if expected_offset != 4 {
            assert_no_event(&mut stream).await;
        }
    }

    let QueryEvent::Completed(completed) = next_event(&mut stream).await else {
        panic!("the typed query must end with query-completed");
    };
    assert_eq!(completed.row_count, 5);
    assert!(!completed.truncated_by_max_rows);
    assert!(!completed.truncated_by_max_result_bytes);
    assert_typed_rows(&rows);

    let unload_error = harness
        .driver_client
        .unload_driver(harness.loaded_driver.driver_id.clone())
        .await
        .expect_err("a driver with an open session must remain loaded");
    assert!(matches!(
        unload_error,
        BridgeError::Remote(error) if error.code == "driver.in_use"
    ));

    session.close().await.expect("H2 session must close");
    assert_eq!(session.state().await, SessionState::Closed);
    harness.finish().await;
}

#[tokio::test]
async fn commits_rolls_back_and_rolls_back_an_active_transaction_on_close() {
    let harness = H2Harness::start("transaction_lifecycle").await;

    let writer = harness.open_session().await;
    execute_update(
        &writer,
        "CREATE TABLE items (id BIGINT PRIMARY KEY, label VARCHAR(64) NOT NULL)",
        Vec::new(),
        None,
    )
    .await;
    let committed = writer
        .begin_transaction(TransactionOptions::default())
        .await
        .expect("transaction must begin");
    assert_eq!(writer.state().await, SessionState::TransactionActive);
    assert_eq!(
        execute_update(
            &writer,
            "INSERT INTO items (id, label) VALUES (?, ?)",
            item_parameters(1, "committed"),
            Some(&committed),
        )
        .await,
        1
    );
    writer
        .commit_transaction(&committed)
        .await
        .expect("transaction must commit");
    assert_eq!(writer.state().await, SessionState::AutoCommit);
    writer.close().await.expect("writer session must close");

    let rollback_session = harness.open_session().await;
    assert_eq!(
        query_items(&rollback_session).await,
        vec![(1, "committed".to_owned())]
    );
    let rolled_back = rollback_session
        .begin_transaction(TransactionOptions::default())
        .await
        .expect("rollback transaction must begin");
    assert_eq!(
        execute_update(
            &rollback_session,
            "INSERT INTO items (id, label) VALUES (?, ?)",
            item_parameters(2, "rolled-back"),
            Some(&rolled_back),
        )
        .await,
        1
    );
    rollback_session
        .rollback_transaction(&rolled_back)
        .await
        .expect("transaction must roll back");
    assert_eq!(rollback_session.state().await, SessionState::AutoCommit);
    rollback_session
        .close()
        .await
        .expect("rollback session must close");

    let close_rollback_session = harness.open_session().await;
    assert_eq!(
        query_items(&close_rollback_session).await,
        vec![(1, "committed".to_owned())]
    );
    let abandoned = close_rollback_session
        .begin_transaction(TransactionOptions::default())
        .await
        .expect("close-rollback transaction must begin");
    assert_eq!(
        execute_update(
            &close_rollback_session,
            "INSERT INTO items (id, label) VALUES (?, ?)",
            item_parameters(3, "close-rolled-back"),
            Some(&abandoned),
        )
        .await,
        1
    );
    close_rollback_session
        .close()
        .await
        .expect("closing an active transaction must roll it back");
    assert_eq!(close_rollback_session.state().await, SessionState::Closed);

    let observer = harness.open_session().await;
    assert_eq!(
        query_items(&observer).await,
        vec![(1, "committed".to_owned())]
    );
    observer.close().await.expect("observer session must close");
    harness.finish().await;
}

#[tokio::test]
async fn cancelling_a_transaction_query_requires_rollback_then_recovers() {
    let harness = H2Harness::start("cancel_recovery").await;
    let session = harness.open_session().await;
    execute_update(
        &session,
        "CREATE TABLE recovered (id BIGINT PRIMARY KEY)",
        Vec::new(),
        None,
    )
    .await;

    let transaction = session
        .begin_transaction(TransactionOptions::default())
        .await
        .expect("transaction must begin");
    let mut stream = session
        .execute_query(QueryRequest {
            sql: "SELECT X FROM SYSTEM_RANGE(1, 1000000)".to_owned(),
            parameters: Vec::new(),
            transaction_id: Some(transaction.id().to_owned()),
            options: QueryOptions {
                max_rows: 0,
                target_batch_rows: 1,
                target_batch_bytes: 1024,
                initial_batch_credits: 0,
                max_result_bytes: MAX_RESULT_BYTES,
            },
        })
        .await
        .expect("transaction query must start");
    assert!(matches!(
        next_event(&mut stream).await,
        QueryEvent::Started(_)
    ));
    assert_no_event(&mut stream).await;

    assert_eq!(
        stream
            .cancel(Some("integration-test cancellation".to_owned()))
            .await
            .expect("active H2 query cancellation must be accepted"),
        CancelDisposition::Accepted
    );
    let terminal_error = tokio::time::timeout(EVENT_TIMEOUT, stream.next_event())
        .await
        .expect("cancelled query must become terminal")
        .expect_err("cancelled query must return a remote error");
    let BridgeError::Remote(remote) = terminal_error else {
        panic!("cancelled query must return a structured remote error: {terminal_error}");
    };
    assert_eq!(remote.code, "database.operation_cancelled");
    assert_eq!(remote.session_state, Some(SessionState::RollbackRequired));
    assert_eq!(session.state().await, SessionState::RollbackRequired);

    let blocked_update = session
        .execute_update(UpdateRequest {
            sql: "INSERT INTO recovered (id) VALUES (1)".to_owned(),
            parameters: Vec::new(),
            transaction_id: Some(transaction.id().to_owned()),
        })
        .await
        .expect_err("rollback-required session must reject another operation");
    assert!(matches!(
        blocked_update,
        BridgeError::Remote(error)
            if error.code == "transaction.rollback_required"
                && error.session_state == Some(SessionState::RollbackRequired)
    ));

    session
        .rollback_transaction(&transaction)
        .await
        .expect("rollback must recover the cancelled transaction");
    assert_eq!(session.state().await, SessionState::AutoCommit);
    assert_eq!(
        execute_update(
            &session,
            "INSERT INTO recovered (id) VALUES (1)",
            Vec::new(),
            None,
        )
        .await,
        1
    );
    assert_eq!(
        query_scalar_i64(&session, "SELECT COUNT(*) FROM recovered").await,
        1
    );

    session.close().await.expect("recovered session must close");
    harness.finish().await;
}

fn required_jar(variable: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(variable).unwrap_or_else(|| panic!("{variable} must point to a JAR")),
    );
    assert!(path.is_file(), "JAR does not exist: {}", path.display());
    path
}

fn expected_driver_id(driver_class: &str, artifacts: &[DriverArtifact]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"chat2db-jdbc-driver-v1\0");
    hasher.update(driver_class.as_bytes());
    hasher.update([0]);
    for artifact in artifacts {
        hasher.update(artifact.sha256());
    }
    let digest = hasher.finalize();
    let mut id = String::with_capacity("sha256:".len() + digest.len() * 2);
    id.push_str("sha256:");
    for byte in digest {
        write!(&mut id, "{byte:02x}").expect("writing to a String cannot fail");
    }
    id
}

async fn next_event(stream: &mut chat2db_java_bridge::QueryStream) -> QueryEvent {
    tokio::time::timeout(EVENT_TIMEOUT, stream.next_event())
        .await
        .expect("query event must arrive before the integration-test timeout")
        .expect("query stream must remain successful")
        .expect("query stream must not close before its terminal event")
}

async fn assert_no_event(stream: &mut chat2db_java_bridge::QueryStream) {
    assert!(
        tokio::time::timeout(NO_EVENT_WINDOW, stream.next_event())
            .await
            .is_err(),
        "a query with no batch credit must remain backpressured"
    );
}

fn assert_typed_rows(rows: &[JdbcRow]) {
    assert_eq!(rows.len(), 5);
    for (index, row) in rows.iter().enumerate() {
        let id = i64::try_from(index + 1).expect("test row index must fit in i64");
        assert_eq!(
            row.values,
            [
                JdbcValue::SignedInteger(id),
                JdbcValue::Text(format!("row-{id}")),
                JdbcValue::Boolean(id % 2 == 0),
                JdbcValue::Decimal(format!("{id}.25")),
            ]
        );
    }
}

fn item_parameters(id: i64, label: &str) -> Vec<JdbcParameter> {
    vec![
        JdbcParameter {
            position: 1,
            value: JdbcValue::SignedInteger(id),
            jdbc_type: None,
            jdbc_type_name: None,
        },
        JdbcParameter {
            position: 2,
            value: JdbcValue::Text(label.to_owned()),
            jdbc_type: None,
            jdbc_type_name: None,
        },
    ]
}

async fn execute_update(
    session: &Session,
    sql: &str,
    parameters: Vec<JdbcParameter>,
    transaction: Option<&Transaction>,
) -> u64 {
    session
        .execute_update(UpdateRequest {
            sql: sql.to_owned(),
            parameters,
            transaction_id: transaction.map(|value| value.id().to_owned()),
        })
        .await
        .expect("H2 update must succeed")
        .affected_rows
}

async fn query_items(session: &Session) -> Vec<(i64, String)> {
    collect_rows(session, "SELECT id, label FROM items ORDER BY id")
        .await
        .into_iter()
        .map(|row| match row.values.as_slice() {
            [JdbcValue::SignedInteger(id), JdbcValue::Text(label)] => (*id, label.clone()),
            values => panic!("unexpected typed item row: {values:?}"),
        })
        .collect()
}

async fn query_scalar_i64(session: &Session, sql: &str) -> i64 {
    let rows = collect_rows(session, sql).await;
    let [row] = rows.as_slice() else {
        panic!("scalar query must return exactly one row");
    };
    let [JdbcValue::SignedInteger(value)] = row.values.as_slice() else {
        panic!("scalar query must return one signed integer");
    };
    *value
}

async fn collect_rows(session: &Session, sql: &str) -> Vec<JdbcRow> {
    let mut stream = session
        .execute_query(QueryRequest {
            sql: sql.to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
            options: QueryOptions {
                max_rows: 0,
                target_batch_rows: 2,
                target_batch_bytes: 1024,
                initial_batch_credits: 8,
                max_result_bytes: MAX_RESULT_BYTES,
            },
        })
        .await
        .expect("H2 verification query must start");
    let mut rows = Vec::new();
    let mut started = false;
    loop {
        match next_event(&mut stream).await {
            QueryEvent::Started(metadata) => {
                assert!(!started, "query metadata must be emitted once");
                assert!(!metadata.columns.is_empty());
                started = true;
            }
            QueryEvent::Batch(batch) => rows.extend(batch.rows),
            QueryEvent::Completed(completed) => {
                assert!(started, "query metadata must precede completion");
                assert_eq!(
                    completed.row_count,
                    u64::try_from(rows.len()).expect("test row count must fit in u64")
                );
                return rows;
            }
        }
    }
}
