use chat2db_storage::{
    AgentCompaction, AgentMessageRole, AgentRunStatus, AgentRunUpdate, AppendAgentMessage,
    CompactAgentRun, CreateAgentSession, CreateOperationLog, CreateProviderProfile,
    CreateSavedConsole, MAX_AGENT_MESSAGE_BYTES, MAX_RESULT_PAGE_BYTES, MAX_RESULT_PAGE_ROWS,
    MIN_RESULT_PAGE_BYTES, OperationLogListQuery, PageRequest, ProviderKind, PurgeReport,
    SavedConsoleListQuery, ToolPermissionDecision, UpdateAgentSession, UpdateSavedConsole,
};

#[test]
fn paging_and_purge_contracts_are_nameable_outside_the_crate() {
    let request = PageRequest {
        offset: 7,
        max_rows: MAX_RESULT_PAGE_ROWS,
        max_bytes: MAX_RESULT_PAGE_BYTES,
    };
    let report = PurgeReport::default();

    assert_eq!(request.offset, 7);
    assert!(request.max_bytes >= MIN_RESULT_PAGE_BYTES);
    assert_eq!(report.results_removed, 0);
}

#[test]
fn saved_console_contracts_are_nameable_outside_the_crate() {
    let create = CreateSavedConsole {
        id: Some(42),
        name: "query".to_owned(),
        data_source_id: Some("opaque-datasource-id".to_owned()),
        data_source_name: Some("Local MySQL".to_owned()),
        database_name: Some("chat2db".to_owned()),
        schema_name: None,
        database_type: Some("MYSQL".to_owned()),
        ddl: "SELECT 1".to_owned(),
        status: "DRAFT".to_owned(),
        tab_opened: "y".to_owned(),
        operation_type: "console".to_owned(),
    };
    let query = SavedConsoleListQuery {
        data_source_id: Some("opaque-datasource-id".to_owned()),
        page_size: 100,
        ..SavedConsoleListQuery::default()
    };
    let update = UpdateSavedConsole {
        schema_name: Some(None),
        ddl: Some("SELECT 1".to_owned()),
        ..UpdateSavedConsole::default()
    };

    assert_eq!(create.id, Some(42));
    assert_eq!(query.page_no, 1);
    assert_eq!(query.page_size, 100);
    assert_eq!(update.schema_name, Some(None));
    assert_eq!(update.ddl.as_deref(), Some("SELECT 1"));
}

#[test]
fn operation_log_contracts_are_nameable_outside_the_crate() {
    let create = CreateOperationLog {
        name: None,
        data_source_id: Some("opaque-datasource-id".to_owned()),
        data_source_name: Some("Local MySQL".to_owned()),
        connectable: Some(true),
        database_name: Some("chat2db".to_owned()),
        database_type: Some("MYSQL".to_owned()),
        ddl: "SELECT 1".to_owned(),
        status: "SUCCESS".to_owned(),
        operation_rows: None,
        use_time: Some(4),
        extend_info: Some(r#"{"source":"console"}"#.to_owned()),
        schema_name: None,
        organization_id: None,
        user_name: None,
        more: false,
        operation_type: "SQL_EXECUTE".to_owned(),
    };
    let query = OperationLogListQuery {
        data_source_id: Some("opaque-datasource-id".to_owned()),
        operation_type: Some("SQL_EXECUTE".to_owned()),
        page_size: 100,
        ..OperationLogListQuery::default()
    };

    assert_eq!(create.connectable, Some(true));
    assert_eq!(create.use_time, Some(4));
    assert_eq!(query.page_no, 1);
    assert_eq!(query.page_size, 100);
}

#[test]
fn provider_and_agent_contracts_are_nameable_outside_the_crate() {
    let provider = CreateProviderProfile {
        name: "primary".to_owned(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://provider.example/v1".to_owned(),
        model: "model-1".to_owned(),
        context_window_tokens: 128_000,
        max_output_tokens: 8_192,
    };
    let session = CreateAgentSession {
        title: "Session".to_owned(),
        provider_id: "provider-id".to_owned(),
        datasource_id: None,
        system_prompt: Some("rules".to_owned()),
    };
    let update = UpdateAgentSession {
        title: "Renamed".to_owned(),
        provider_id: "provider-id".to_owned(),
        datasource_id: Some("datasource-id".to_owned()),
    };
    let message = AppendAgentMessage {
        role: AgentMessageRole::User,
        summary_through_ordinal: None,
        content_json: "[{\"type\":\"text\",\"text\":\"hello\"}]".to_owned(),
    };
    let run = AgentRunUpdate {
        status: AgentRunStatus::Running,
        last_sequence: 2,
        model_rounds: 1,
        tool_calls: 0,
        input_tokens: 10,
        output_tokens: 2,
        total_tokens: 12,
        compaction_count: 0,
        compacted_through_ordinal: None,
    };
    let compaction = CompactAgentRun {
        last_sequence: 2,
        model_rounds: 0,
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        compaction: AgentCompaction::DeterministicTrim {
            compacted_through_ordinal: 1,
        },
    };

    assert_eq!(provider.max_output_tokens, 8_192);
    assert!(session.system_prompt.is_some());
    assert!(update.datasource_id.is_some());
    assert_eq!(message.role, AgentMessageRole::User);
    assert_eq!(run.status, AgentRunStatus::Running);
    assert!(matches!(
        compaction.compaction,
        AgentCompaction::DeterministicTrim {
            compacted_through_ordinal: 1
        }
    ));
    assert_eq!(
        ToolPermissionDecision::Approve,
        ToolPermissionDecision::Approve
    );
    assert!(
        u64::try_from(message.content_json.len()).expect("message length fits")
            < MAX_AGENT_MESSAGE_BYTES
    );
}
