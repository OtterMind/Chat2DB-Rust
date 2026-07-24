use chat2db_storage::{
    AgentMessageRole, AgentRunStatus, AgentRunUpdate, AppendAgentMessage, CreateAgentSession,
    CreateProviderProfile, MAX_AGENT_MESSAGE_BYTES, MAX_RESULT_PAGE_BYTES, MAX_RESULT_PAGE_ROWS,
    MIN_RESULT_PAGE_BYTES, PageRequest, ProviderKind, PurgeReport, ToolPermissionDecision,
    UpdateAgentSession,
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
        run_id: None,
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

    assert_eq!(provider.max_output_tokens, 8_192);
    assert!(session.system_prompt.is_some());
    assert!(update.datasource_id.is_some());
    assert_eq!(message.role, AgentMessageRole::User);
    assert_eq!(run.status, AgentRunStatus::Running);
    assert_eq!(
        ToolPermissionDecision::Approve,
        ToolPermissionDecision::Approve
    );
    assert!(
        u64::try_from(message.content_json.len()).expect("message length fits")
            < MAX_AGENT_MESSAGE_BYTES
    );
}
