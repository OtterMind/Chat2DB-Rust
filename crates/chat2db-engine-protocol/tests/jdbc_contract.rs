use chat2db_engine_protocol::{current_version, wire};
use prost::Message;

fn request_meta(request_id: &str) -> wire::RequestMeta {
    wire::RequestMeta {
        request_id: request_id.to_owned(),
        trace_id: "trace-jdbc-1".to_owned(),
        session_id: Some("session-1".to_owned()),
        deadline_unix_millis: Some(1_900_000_000_000),
        cancellation_id: Some("cancel-1".to_owned()),
    }
}

fn response_meta(sequence: u64, terminal: bool) -> wire::ResponseMeta {
    wire::ResponseMeta {
        request_id: "query-1".to_owned(),
        trace_id: "trace-jdbc-1".to_owned(),
        sequence,
        terminal,
    }
}

fn envelope_field_number<M>(message: &M) -> u32
where
    M: Message,
{
    let encoded = message.encode_to_vec();
    let mut key = 0_u64;
    for (index, byte) in encoded.into_iter().enumerate() {
        let shift = u32::try_from(index * 7).expect("field key shift must fit u32");
        key |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return u32::try_from(key >> 3).expect("field number must fit u32");
        }
    }
    panic!("encoded envelope must contain a field key");
}

#[test]
fn jdbc_query_input_round_trips_with_typed_parameters() {
    let request = wire::ClientEnvelope {
        meta: Some(request_meta("query-1")),
        payload: Some(wire::client_envelope::Payload::ExecuteQuery(
            wire::ExecuteQueryRequest {
                sql: "select name from users where id = ? and active = ?".to_owned(),
                parameters: vec![
                    wire::JdbcParameter {
                        position: 1,
                        value: Some(wire::JdbcValue {
                            value: Some(wire::jdbc_value::Value::SignedIntegerValue(42)),
                        }),
                        jdbc_type: Some(-5),
                        jdbc_type_name: Some("BIGINT".to_owned()),
                    },
                    wire::JdbcParameter {
                        position: 2,
                        value: Some(wire::JdbcValue {
                            value: Some(wire::jdbc_value::Value::BooleanValue(true)),
                        }),
                        jdbc_type: Some(16),
                        jdbc_type_name: Some("BOOLEAN".to_owned()),
                    },
                ],
                transaction_id: Some("transaction-1".to_owned()),
                options: Some(wire::QueryOptions {
                    max_rows: 10_000,
                    target_batch_rows: 512,
                    target_batch_bytes: 1_048_576,
                    initial_batch_credits: 2,
                    max_result_bytes: 64 * 1_048_576,
                }),
            },
        )),
    };

    let decoded = wire::ClientEnvelope::decode(request.encode_to_vec().as_slice())
        .expect("JDBC request must decode");

    assert_eq!(decoded, request);
    assert_eq!(
        current_version(),
        wire::ProtocolVersion { major: 1, minor: 1 }
    );
}

#[test]
fn query_stream_uses_envelope_identity_and_contiguous_sequence() {
    let column = wire::JdbcColumn {
        ordinal: 1,
        label: "amount".to_owned(),
        name: "amount".to_owned(),
        jdbc_type: 3,
        jdbc_type_name: "DECIMAL".to_owned(),
        value_type: wire::JdbcValueType::Decimal as i32,
        nullability: wire::ColumnNullability::Nullable as i32,
        precision: Some(18),
        scale: Some(2),
        ..Default::default()
    };
    let row = wire::JdbcRow {
        values: vec![wire::JdbcValue {
            value: Some(wire::jdbc_value::Value::DecimalValue(
                "1234567890.25".to_owned(),
            )),
        }],
    };
    let responses = [
        wire::ServerEnvelope {
            meta: Some(response_meta(0, false)),
            payload: Some(wire::server_envelope::Payload::QueryStarted(
                wire::QueryStarted {
                    columns: vec![column],
                },
            )),
        },
        wire::ServerEnvelope {
            meta: Some(response_meta(1, false)),
            payload: Some(wire::server_envelope::Payload::RowBatch(wire::RowBatch {
                start_row_offset: 0,
                rows: vec![row],
            })),
        },
        wire::ServerEnvelope {
            meta: Some(response_meta(2, true)),
            payload: Some(wire::server_envelope::Payload::QueryCompleted(
                wire::QueryCompleted {
                    row_count: 1,
                    truncated_by_max_rows: false,
                    truncated_by_max_result_bytes: false,
                },
            )),
        },
    ];

    for (expected_sequence, response) in responses.iter().enumerate() {
        let decoded = wire::ServerEnvelope::decode(response.encode_to_vec().as_slice())
            .expect("query response must decode");
        let meta = decoded.meta.expect("query response meta is required");
        assert_eq!(meta.request_id, "query-1");
        assert_eq!(meta.trace_id, "trace-jdbc-1");
        assert_eq!(meta.sequence, expected_sequence as u64);
        assert_eq!(meta.terminal, expected_sequence == responses.len() - 1);
    }
}

#[test]
fn jdbc_client_envelope_field_numbers_are_stable() {
    let client_payloads = [
        (
            wire::client_envelope::Payload::LoadDriver(wire::LoadDriverRequest::default()),
            100,
        ),
        (
            wire::client_envelope::Payload::UnloadDriver(wire::UnloadDriverRequest::default()),
            101,
        ),
        (
            wire::client_envelope::Payload::OpenSession(wire::OpenSessionRequest::default()),
            110,
        ),
        (
            wire::client_envelope::Payload::CloseSession(wire::CloseSessionRequest::default()),
            111,
        ),
        (
            wire::client_envelope::Payload::BeginTransaction(
                wire::BeginTransactionRequest::default(),
            ),
            120,
        ),
        (
            wire::client_envelope::Payload::CommitTransaction(
                wire::CommitTransactionRequest::default(),
            ),
            121,
        ),
        (
            wire::client_envelope::Payload::RollbackTransaction(
                wire::RollbackTransactionRequest::default(),
            ),
            122,
        ),
        (
            wire::client_envelope::Payload::ExecuteQuery(wire::ExecuteQueryRequest::default()),
            130,
        ),
        (
            wire::client_envelope::Payload::ExecuteUpdate(wire::ExecuteUpdateRequest::default()),
            131,
        ),
        (
            wire::client_envelope::Payload::GrantCredits(wire::GrantCreditsRequest::default()),
            132,
        ),
        (
            wire::client_envelope::Payload::CancelOperation(wire::CancelOperationRequest::default()),
            133,
        ),
    ];
    for (payload, expected_field) in client_payloads {
        let envelope = wire::ClientEnvelope {
            meta: None,
            payload: Some(payload),
        };
        assert_eq!(envelope_field_number(&envelope), expected_field);
    }
}

#[test]
fn jdbc_server_envelope_field_numbers_are_stable() {
    let server_payloads = [
        (
            wire::server_envelope::Payload::DriverLoaded(wire::DriverLoaded::default()),
            100,
        ),
        (
            wire::server_envelope::Payload::DriverUnloaded(wire::DriverUnloaded::default()),
            101,
        ),
        (
            wire::server_envelope::Payload::SessionOpened(wire::SessionOpened::default()),
            110,
        ),
        (
            wire::server_envelope::Payload::SessionClosed(wire::SessionClosed::default()),
            111,
        ),
        (
            wire::server_envelope::Payload::TransactionStarted(wire::TransactionStarted::default()),
            120,
        ),
        (
            wire::server_envelope::Payload::TransactionCommitted(
                wire::TransactionCommitted::default(),
            ),
            121,
        ),
        (
            wire::server_envelope::Payload::TransactionRolledBack(
                wire::TransactionRolledBack::default(),
            ),
            122,
        ),
        (
            wire::server_envelope::Payload::QueryStarted(wire::QueryStarted::default()),
            130,
        ),
        (
            wire::server_envelope::Payload::RowBatch(wire::RowBatch::default()),
            131,
        ),
        (
            wire::server_envelope::Payload::QueryCompleted(wire::QueryCompleted::default()),
            132,
        ),
        (
            wire::server_envelope::Payload::UpdateCompleted(wire::UpdateCompleted::default()),
            133,
        ),
        (
            wire::server_envelope::Payload::CreditsGranted(wire::CreditsGranted::default()),
            134,
        ),
        (
            wire::server_envelope::Payload::OperationCancelled(wire::OperationCancelled::default()),
            135,
        ),
    ];
    for (payload, expected_field) in server_payloads {
        let envelope = wire::ServerEnvelope {
            meta: None,
            payload: Some(payload),
        };
        assert_eq!(envelope_field_number(&envelope), expected_field);
    }
}

#[test]
fn jdbc_hard_limits_are_generated_from_the_shared_schema() {
    let limits = [
        (wire::JdbcProtocolLimit::MaxCreditGrant, 8),
        (wire::JdbcProtocolLimit::MaxErrorCauses, 16),
        (wire::JdbcProtocolLimit::MaxDriverArtifacts, 32),
        (wire::JdbcProtocolLimit::MaxConnectionProperties, 128),
        (wire::JdbcProtocolLimit::MaxDriverIdBytes, 255),
        (wire::JdbcProtocolLimit::MaxPropertyKeyBytes, 256),
        (wire::JdbcProtocolLimit::MaxDriverClassBytes, 512),
        (wire::JdbcProtocolLimit::MaxColumns, 2048),
        (wire::JdbcProtocolLimit::MaxBatchRows, 4096),
        (wire::JdbcProtocolLimit::MaxPathBytes, 8192),
        (wire::JdbcProtocolLimit::MaxParameters, 32_768),
        (wire::JdbcProtocolLimit::MaxJdbcUrlBytes, 65_536),
        (wire::JdbcProtocolLimit::MaxPropertyValueBytes, 262_144),
        (wire::JdbcProtocolLimit::MaxSqlBytes, 1_048_576),
        (wire::JdbcProtocolLimit::MaxScalarBytes, 4_194_304),
        (wire::JdbcProtocolLimit::MaxBatchBytes, 8_388_608),
        (wire::JdbcProtocolLimit::MaxDriverArtifactBytes, 268_435_456),
        (wire::JdbcProtocolLimit::MaxDriverTotalBytes, 1_073_741_824),
    ];

    for (limit, expected) in limits {
        assert_eq!(limit as i32, expected);
        assert_eq!(
            wire::JdbcProtocolLimit::from_str_name(limit.as_str_name()),
            Some(limit)
        );
    }

    assert_eq!(
        wire::JdbcCreditWindowLimit::MaxOutstandingCredits as i32,
        32
    );
    assert_eq!(
        wire::JdbcCreditWindowLimit::from_str_name(
            wire::JdbcCreditWindowLimit::MaxOutstandingCredits.as_str_name(),
        ),
        Some(wire::JdbcCreditWindowLimit::MaxOutstandingCredits)
    );
    assert_eq!(
        wire::JdbcResultByteLimit::DefaultResultBytes as i32,
        64 * 1_048_576
    );
    assert_eq!(
        wire::JdbcResultByteLimit::MaxResultBytes as i32,
        1_024 * 1_048_576
    );
}

#[test]
fn driver_identity_is_engine_owned_and_session_state_is_explicit() {
    let load = wire::LoadDriverRequest {
        driver_class: "org.h2.Driver".to_owned(),
        artifacts: vec![wire::DriverArtifact {
            path: "/drivers/h2.jar".to_owned(),
            sha256: vec![0x2a; 32],
        }],
    };
    let decoded = wire::LoadDriverRequest::decode(load.encode_to_vec().as_slice())
        .expect("load driver request must decode");
    assert_eq!(decoded, load);

    let states = [
        wire::SessionState::AutoCommit,
        wire::SessionState::TransactionActive,
        wire::SessionState::RollbackRequired,
        wire::SessionState::Broken,
        wire::SessionState::Closed,
    ];
    for state in states {
        assert_eq!(
            wire::SessionState::from_str_name(state.as_str_name()),
            Some(state)
        );
    }
}

#[test]
fn database_error_detail_round_trips_without_string_metadata() {
    let error = wire::EngineError {
        code: "database.constraint_violation".to_owned(),
        message: "duplicate key".to_owned(),
        category: wire::ErrorCategory::Database as i32,
        retryable: false,
        fatal: false,
        outcome: wire::OperationOutcome::KnownFailed as i32,
        metadata: std::collections::HashMap::new(),
        database_error: Some(wire::DatabaseErrorDetail {
            sql_state: Some("23505".to_owned()),
            vendor_code: Some(23_505),
            constraint_name: Some("users_email_key".to_owned()),
            statement_position: None,
            causes: vec![wire::DatabaseErrorCause {
                class_name: "org.h2.jdbc.JdbcSQLIntegrityConstraintViolationException".to_owned(),
                message: "unique index or primary key violation".to_owned(),
                sql_state: Some("23505".to_owned()),
                vendor_code: Some(23_505),
            }],
        }),
        session_state: Some(wire::SessionState::RollbackRequired as i32),
    };

    let decoded = wire::EngineError::decode(error.encode_to_vec().as_slice())
        .expect("database error must decode");
    assert_eq!(decoded, error);
}
