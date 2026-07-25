use std::{future::Future, ops::Range};

use chat2db_agent::{CompactionEvent, CompactionStrategy, Usage};
use chat2db_contract::{
    AgentEvent, AgentMessageContent, AgentRunSnapshot, AgentRunStatus as ContractRunStatus,
    AgentUsage, ContextCompactionStrategy,
};
use chat2db_storage::{
    AgentCompaction, AgentMessageRole, AgentRunRecord, AgentRunStatus as StorageRunStatus,
    CompactAgentRun, CompactedAgentRun, Storage, StorageError,
};

use super::{
    hub::{AgentRunHub, AgentTransitionFailure, DurableAgentTransition},
    transcript::CompactionOrdinalMap,
};
use crate::AppError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RunProgress {
    pub model_rounds: u64,
    pub tool_calls: u64,
    pub usage: Usage,
}

/// A compaction event mapped exactly once against the pre-compaction transcript.
///
/// Retrying durable persistence reuses this value. It must not apply the same
/// message range to [`CompactionOrdinalMap`] a second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MappedCompaction {
    storage: AgentCompaction,
    event: AgentEvent,
}

impl MappedCompaction {
    pub(super) const fn durable_effect(&self) -> (bool, Option<u64>) {
        match self.storage {
            AgentCompaction::NoOp => (false, None),
            AgentCompaction::DeterministicTrim {
                compacted_through_ordinal,
            }
            | AgentCompaction::Summary {
                compacted_through_ordinal,
                ..
            } => (true, Some(compacted_through_ordinal)),
        }
    }
}

enum CompactionCommitFailure {
    Storage(StorageError),
    Indeterminate(AppError),
}

pub(super) fn map_context_compaction(
    ordinals: &mut CompactionOrdinalMap,
    compaction: &CompactionEvent,
) -> Result<MappedCompaction, AppError> {
    map_compaction_parts(
        ordinals,
        compaction.strategy,
        compaction.removed_turns,
        compaction.replacement_summary(),
        compaction.compacted_message_range(),
    )
}

fn map_compaction_parts(
    ordinals: &mut CompactionOrdinalMap,
    strategy: CompactionStrategy,
    removed_turns: usize,
    replacement_summary: Option<&str>,
    compacted_message_range: Option<Range<usize>>,
) -> Result<MappedCompaction, AppError> {
    let summary_json = match strategy {
        CompactionStrategy::Summary
            if removed_turns > 0
                && compacted_message_range.is_some()
                && replacement_summary.is_some_and(|summary| !summary.trim().is_empty()) =>
        {
            let summary = replacement_summary.ok_or_else(AppError::internal)?;
            Some(
                serde_json::to_string(&[AgentMessageContent::Text {
                    text: summary.to_owned(),
                }])
                .map_err(|_| AppError::internal())?,
            )
        }
        CompactionStrategy::Summary => return Err(AppError::internal()),
        CompactionStrategy::DeterministicTrim
            if replacement_summary.is_some()
                || (removed_turns == 0) != compacted_message_range.is_none() =>
        {
            return Err(AppError::internal());
        }
        CompactionStrategy::DeterministicTrim => None,
    };

    let coverage = ordinals.apply_compaction(compacted_message_range, strategy)?;
    let storage = match (strategy, coverage, summary_json) {
        (CompactionStrategy::DeterministicTrim, None, None) => AgentCompaction::NoOp,
        (CompactionStrategy::DeterministicTrim, Some(compacted_through_ordinal), None) => {
            AgentCompaction::DeterministicTrim {
                compacted_through_ordinal,
            }
        }
        (CompactionStrategy::Summary, Some(compacted_through_ordinal), Some(content_json)) => {
            AgentCompaction::Summary {
                compacted_through_ordinal,
                content_json,
            }
        }
        _ => return Err(AppError::internal()),
    };
    let event = AgentEvent::ContextCompacted {
        strategy: match strategy {
            CompactionStrategy::Summary => ContextCompactionStrategy::Summary,
            CompactionStrategy::DeterministicTrim => ContextCompactionStrategy::DeterministicTrim,
        },
        dropped_turns: removed_turns.to_string(),
    };
    Ok(MappedCompaction { storage, event })
}

/// Commits one mapped context-compaction event before making it observable.
///
/// # Errors
///
/// Returns a classified storage or Hub error. Unknown commit outcomes discard
/// the process-local Hub entry so the allocated sequence cannot be reused.
pub(super) async fn persist_context_compaction(
    hub: &AgentRunHub,
    storage: Storage,
    run_id: &str,
    progress: RunProgress,
    mapped: MappedCompaction,
) -> Result<AgentRunSnapshot, AppError> {
    persist_context_compaction_with(
        hub,
        run_id,
        progress,
        mapped,
        move |run_id, input| async move {
            tokio::task::spawn_blocking(move || {
                storage.compact_agent_run(&run_id, StorageRunStatus::Running, input)
            })
            .await
            .map_err(|_| CompactionCommitFailure::Indeterminate(AppError::internal()))?
            .map_err(CompactionCommitFailure::Storage)
        },
    )
    .await
}

async fn persist_context_compaction_with<F, Fut>(
    hub: &AgentRunHub,
    run_id: &str,
    progress: RunProgress,
    mapped: MappedCompaction,
    commit: F,
) -> Result<AgentRunSnapshot, AppError>
where
    F: FnOnce(String, CompactAgentRun) -> Fut,
    Fut: Future<Output = Result<CompactedAgentRun, CompactionCommitFailure>>,
{
    let persisted_event = mapped.event.clone();
    let expected_compaction = mapped.storage.clone();
    let run_id_owned = run_id.to_owned();
    hub.transition(run_id, move |sequence| {
        let input = CompactAgentRun {
            last_sequence: sequence,
            model_rounds: progress.model_rounds,
            tool_calls: progress.tool_calls,
            input_tokens: progress.usage.input_tokens,
            output_tokens: progress.usage.output_tokens,
            total_tokens: progress.usage.total_tokens,
            compaction: mapped.storage,
        };
        async move {
            let compacted = commit(run_id_owned, input)
                .await
                .map_err(transition_failure)?;
            validate_compacted_effect(&expected_compaction, &compacted)
                .map_err(AgentTransitionFailure::indeterminate)?;
            let snapshot =
                running_snapshot(compacted.run).map_err(AgentTransitionFailure::indeterminate)?;
            Ok(DurableAgentTransition::new(snapshot, persisted_event))
        }
    })
    .await
}

fn transition_failure(failure: CompactionCommitFailure) -> AgentTransitionFailure {
    match failure {
        CompactionCommitFailure::Storage(error) => {
            let outcome_unknown = matches!(error, StorageError::OutcomeUnknown { .. });
            let error = AppError::from(error);
            if outcome_unknown {
                AgentTransitionFailure::indeterminate(error)
            } else {
                AgentTransitionFailure::definitely_not_committed(error)
            }
        }
        CompactionCommitFailure::Indeterminate(error) => {
            AgentTransitionFailure::indeterminate(error)
        }
    }
}

fn validate_compacted_effect(
    expected: &AgentCompaction,
    compacted: &CompactedAgentRun,
) -> Result<(), AppError> {
    let valid = match (expected, compacted.summary_message.as_ref()) {
        (AgentCompaction::NoOp, None) => true,
        (
            AgentCompaction::DeterministicTrim {
                compacted_through_ordinal,
            },
            None,
        ) => compacted.run.compacted_through_ordinal == Some(*compacted_through_ordinal),
        (
            AgentCompaction::Summary {
                compacted_through_ordinal,
                content_json,
            },
            Some(summary),
        ) => {
            compacted.run.compacted_through_ordinal == Some(*compacted_through_ordinal)
                && summary.run_id.as_deref() == Some(compacted.run.id.as_str())
                && summary.role == AgentMessageRole::Summary
                && summary.summary_through_ordinal == Some(*compacted_through_ordinal)
                && summary.content_json == *content_json
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::internal())
    }
}

fn running_snapshot(run: AgentRunRecord) -> Result<AgentRunSnapshot, AppError> {
    if run.status != StorageRunStatus::Running
        || run.cancel_requested
        || run.write_in_flight_tool_call_id.is_some()
        || run.write_in_flight_arguments_sha256.is_some()
        || run.message_id.is_some()
        || run.error_code.is_some()
        || run.error_message.is_some()
        || run.finished_at_ms.is_some()
    {
        return Err(AppError::internal());
    }
    let started_at_ms = run.started_at_ms.ok_or_else(AppError::internal)?;
    Ok(AgentRunSnapshot {
        run_id: run.id,
        session_id: run.session_id,
        status: ContractRunStatus::Running,
        last_sequence: run.last_sequence.to_string(),
        started_at_ms: started_at_ms.to_string(),
        updated_at_ms: run.updated_at_ms.to_string(),
        model_rounds: run.model_rounds.to_string(),
        tool_calls: run.tool_calls.to_string(),
        usage: AgentUsage {
            input_tokens: run.input_tokens.to_string(),
            output_tokens: run.output_tokens.to_string(),
            total_tokens: run.total_tokens.to_string(),
        },
        pending_permission: None,
        message_id: None,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use chat2db_contract::{AgentEvent, ContextCompactionStrategy};
    use chat2db_storage::{
        AgentMessageRole, AgentRunStatus as StorageRunStatus, AppendAgentMessage,
        CreateAgentSession, CreateProviderProfile, ProviderKind, SecretRef, SecretValue,
        SecretVault, SecretVaultError, SqlPermissionMode, StartAgentRun, Storage,
    };
    use tempfile::TempDir;
    use tokio::{sync::oneshot, time::Duration};

    use super::{
        CompactionCommitFailure, CompactionOrdinalMap, CompactionStrategy, MappedCompaction,
        RunProgress, map_compaction_parts, persist_context_compaction,
        persist_context_compaction_with, running_snapshot,
    };
    use crate::{AppError, AppErrorKind, agent::hub::AgentRunHub};

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

    struct Fixture {
        _directory: TempDir,
        storage: Storage,
        run: chat2db_storage::AgentRunRecord,
        session_id: String,
    }

    fn setup() -> Fixture {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens");
        let provider = storage
            .create_provider_profile(
                CreateProviderProfile {
                    name: "primary".to_owned(),
                    kind: ProviderKind::OpenAiCompatible,
                    base_url: "https://provider.example/v1".to_owned(),
                    model: "model-1".to_owned(),
                    context_window_tokens: 128_000,
                    max_output_tokens: 8_192,
                },
                None,
            )
            .expect("provider creates");
        let session = storage
            .create_agent_session(CreateAgentSession {
                title: "Session".to_owned(),
                provider_id: provider.id,
                datasource_id: None,
                system_prompt: None,
            })
            .expect("session creates");
        for (role, text) in [
            (AgentMessageRole::User, "previous question"),
            (AgentMessageRole::Assistant, "previous answer"),
        ] {
            storage
                .append_agent_message(
                    &session.id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: format!(r#"[{{"type":"text","text":"{text}"}}]"#),
                    },
                )
                .expect("history appends");
        }
        let run = storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "current question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            )
            .expect("run starts")
            .run;
        Fixture {
            _directory: directory,
            storage,
            run,
            session_id: session.id,
        }
    }

    async fn register(hub: &AgentRunHub, run: &chat2db_storage::AgentRunRecord) {
        hub.reserve()
            .await
            .expect("capacity reserves")
            .register_started(running_snapshot(run.clone()).expect("snapshot converts"))
            .expect("run registers");
    }

    fn summary_mapping() -> (CompactionOrdinalMap, MappedCompaction) {
        let mut ordinals = CompactionOrdinalMap::new([0, 1, 2]);
        let mapped = map_compaction_parts(
            &mut ordinals,
            CompactionStrategy::Summary,
            1,
            Some("bounded summary"),
            Some(0..2),
        )
        .expect("summary maps");
        (ordinals, mapped)
    }

    #[test]
    fn mapping_is_single_use_and_preserves_exact_ordinals_across_modes() {
        let mut ordinals = CompactionOrdinalMap::new([0, 4, 8, 10, 12]);
        let summary = map_compaction_parts(
            &mut ordinals,
            CompactionStrategy::Summary,
            1,
            Some("bounded summary"),
            Some(1..3),
        )
        .expect("summary maps");
        assert!(matches!(
            summary.event,
            AgentEvent::ContextCompacted {
                strategy: ContextCompactionStrategy::Summary,
                ref dropped_turns,
            } if dropped_turns == "1"
        ));

        ordinals
            .append_transient_messages(2)
            .expect("transient messages append");
        let trim = map_compaction_parts(
            &mut ordinals,
            CompactionStrategy::DeterministicTrim,
            1,
            None,
            Some(2..4),
        )
        .expect("later durable range maps");
        assert!(matches!(
            trim.storage,
            chat2db_storage::AgentCompaction::DeterministicTrim {
                compacted_through_ordinal: 12
            }
        ));
        assert!(
            map_compaction_parts(
                &mut ordinals,
                CompactionStrategy::DeterministicTrim,
                1,
                None,
                Some(1..2),
            )
            .is_err(),
            "a transient summary slot must fail closed"
        );
    }

    #[test]
    fn invalid_compaction_metadata_does_not_mutate_the_ordinal_map() {
        let cases = [
            (CompactionStrategy::Summary, 1, None, Some(0..2)),
            (CompactionStrategy::Summary, 0, Some("summary"), None),
            (CompactionStrategy::DeterministicTrim, 0, None, Some(0..2)),
            (
                CompactionStrategy::DeterministicTrim,
                1,
                Some("unexpected"),
                Some(0..2),
            ),
        ];
        for (strategy, removed_turns, summary, range) in cases {
            let mut ordinals = CompactionOrdinalMap::new([0, 1, 2]);
            assert!(
                map_compaction_parts(&mut ordinals, strategy, removed_turns, summary, range,)
                    .is_err()
            );
            assert_eq!(
                ordinals
                    .apply_compaction(Some(0..2), CompactionStrategy::DeterministicTrim)
                    .expect("original map remains usable"),
                Some(1)
            );
        }
    }

    #[tokio::test]
    async fn summary_commits_before_the_hub_publishes_its_exact_sequence() {
        let fixture = setup();
        let hub = AgentRunHub::new();
        register(&hub, &fixture.run).await;
        let mut subscription = hub
            .subscribe(&fixture.run.id, Some(1))
            .await
            .expect("subscription opens");
        let (_ordinals, mapped) = summary_mapping();

        let snapshot = persist_context_compaction(
            &hub,
            fixture.storage.clone(),
            &fixture.run.id,
            RunProgress::default(),
            mapped,
        )
        .await
        .expect("summary persists and publishes");
        assert_eq!(snapshot.last_sequence, "2");
        let durable = fixture
            .storage
            .get_agent_run(&fixture.run.id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(durable.last_sequence, 2);
        assert_eq!(durable.compaction_count, 1);
        assert_eq!(durable.compacted_through_ordinal, Some(1));
        let summaries = fixture
            .storage
            .list_agent_messages(&fixture.session_id, 0, 10)
            .expect("messages list")
            .into_iter()
            .filter(|message| message.role == AgentMessageRole::Summary)
            .collect::<Vec<_>>();
        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0].run_id.as_deref(),
            Some(fixture.run.id.as_str())
        );
        assert_eq!(summaries[0].summary_through_ordinal, Some(1));

        let published = subscription
            .next_event()
            .await
            .expect("event reads")
            .expect("event exists");
        assert_eq!(published.sequence, "2");
        assert!(matches!(
            published.event,
            AgentEvent::ContextCompacted {
                strategy: ContextCompactionStrategy::Summary,
                dropped_turns,
            } if dropped_turns == "1"
        ));
    }

    #[tokio::test]
    async fn pending_commit_cannot_publish_a_compaction_event() {
        let fixture = setup();
        let hub = AgentRunHub::new();
        register(&hub, &fixture.run).await;
        let mut subscription = hub
            .subscribe(&fixture.run.id, Some(1))
            .await
            .expect("subscription opens");
        let (_ordinals, mapped) = summary_mapping();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let task_hub = hub.clone();
        let task_storage = fixture.storage.clone();
        let task_run_id = fixture.run.id.clone();
        let persist = tokio::spawn(async move {
            persist_context_compaction_with(
                &task_hub,
                &task_run_id,
                RunProgress::default(),
                mapped,
                move |run_id, input| async move {
                    entered_tx.send(()).map_err(|()| {
                        CompactionCommitFailure::Indeterminate(AppError::internal())
                    })?;
                    release_rx.await.map_err(|_| {
                        CompactionCommitFailure::Indeterminate(AppError::internal())
                    })?;
                    task_storage
                        .compact_agent_run(&run_id, StorageRunStatus::Running, input)
                        .map_err(CompactionCommitFailure::Storage)
                },
            )
            .await
        });
        entered_rx.await.expect("commit future starts");

        assert_eq!(
            fixture
                .storage
                .get_agent_run(&fixture.run.id)
                .expect("run reloads")
                .expect("run exists")
                .last_sequence,
            1
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), subscription.next_event())
                .await
                .is_err(),
            "no event may publish while the durable commit is pending"
        );

        release_tx.send(()).expect("commit releases");
        let snapshot = persist
            .await
            .expect("persistence task joins")
            .expect("compaction persists");
        assert_eq!(snapshot.last_sequence, "2");
        assert_eq!(
            fixture
                .storage
                .get_agent_run(&fixture.run.id)
                .expect("run reloads")
                .expect("run exists")
                .last_sequence,
            2
        );
        let event = subscription
            .next_event()
            .await
            .expect("event reads")
            .expect("event exists");
        assert_eq!(event.sequence, "2");
        assert!(matches!(
            event.event,
            AgentEvent::ContextCompacted {
                strategy: ContextCompactionStrategy::Summary,
                dropped_turns,
            } if dropped_turns == "1"
        ));
    }

    #[tokio::test]
    async fn no_op_and_trim_publish_only_their_durable_effects() {
        let fixture = setup();
        let hub = AgentRunHub::new();
        register(&hub, &fixture.run).await;
        let mut subscription = hub
            .subscribe(&fixture.run.id, Some(1))
            .await
            .expect("subscription opens");
        let mut ordinals = CompactionOrdinalMap::new([0, 1, 2]);
        let no_op = map_compaction_parts(
            &mut ordinals,
            CompactionStrategy::DeterministicTrim,
            0,
            None,
            None,
        )
        .expect("no-op maps");
        persist_context_compaction(
            &hub,
            fixture.storage.clone(),
            &fixture.run.id,
            RunProgress::default(),
            no_op,
        )
        .await
        .expect("no-op persists");
        let after_no_op = fixture
            .storage
            .get_agent_run(&fixture.run.id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(after_no_op.last_sequence, 2);
        assert_eq!(after_no_op.compaction_count, 0);
        assert_eq!(after_no_op.compacted_through_ordinal, None);

        let trim = map_compaction_parts(
            &mut ordinals,
            CompactionStrategy::DeterministicTrim,
            1,
            None,
            Some(0..2),
        )
        .expect("trim maps");
        persist_context_compaction(
            &hub,
            fixture.storage.clone(),
            &fixture.run.id,
            RunProgress::default(),
            trim,
        )
        .await
        .expect("trim persists");
        let after_trim = fixture
            .storage
            .get_agent_run(&fixture.run.id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(after_trim.last_sequence, 3);
        assert_eq!(after_trim.compaction_count, 1);
        assert_eq!(after_trim.compacted_through_ordinal, Some(1));
        assert_eq!(
            fixture
                .storage
                .list_agent_messages(&fixture.session_id, 0, 10)
                .expect("messages list")
                .len(),
            3
        );
        for (sequence, turns) in [("2", "0"), ("3", "1")] {
            let event = subscription
                .next_event()
                .await
                .expect("event reads")
                .expect("event exists");
            assert_eq!(event.sequence, sequence);
            assert!(matches!(
                event.event,
                AgentEvent::ContextCompacted {
                    strategy: ContextCompactionStrategy::DeterministicTrim,
                    dropped_turns,
                } if dropped_turns == turns
            ));
        }
    }

    #[tokio::test]
    async fn definite_failure_reuses_the_sequence_and_the_same_mapping() {
        let fixture = setup();
        let hub = AgentRunHub::new();
        register(&hub, &fixture.run).await;
        let mut subscription = hub
            .subscribe(&fixture.run.id, Some(1))
            .await
            .expect("subscription opens");
        let (_ordinals, mapped) = summary_mapping();
        let invalid_progress = RunProgress {
            model_rounds: 0,
            tool_calls: 0,
            usage: chat2db_agent::Usage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 1,
            },
        };
        let error = persist_context_compaction(
            &hub,
            fixture.storage.clone(),
            &fixture.run.id,
            invalid_progress,
            mapped.clone(),
        )
        .await
        .expect_err("invalid progress is definitely not committed");
        assert_eq!(error.kind(), AppErrorKind::InvalidRequest);
        assert_eq!(
            hub.cached_snapshot(&fixture.run.id)
                .await
                .expect("hub remains available")
                .last_sequence,
            "1"
        );
        assert_eq!(
            fixture
                .storage
                .get_agent_run(&fixture.run.id)
                .expect("run reloads")
                .expect("run exists")
                .last_sequence,
            1
        );

        persist_context_compaction(
            &hub,
            fixture.storage.clone(),
            &fixture.run.id,
            RunProgress::default(),
            mapped,
        )
        .await
        .expect("same mapped event retries once");
        assert_eq!(
            subscription
                .next_event()
                .await
                .expect("event reads")
                .expect("event exists")
                .sequence,
            "2"
        );
    }

    #[tokio::test]
    async fn unknown_outcome_invalidates_the_hub_without_publishing() {
        let fixture = setup();
        let hub = AgentRunHub::new();
        register(&hub, &fixture.run).await;
        let mut subscription = hub
            .subscribe(&fixture.run.id, Some(1))
            .await
            .expect("subscription opens");
        let mut ordinals = CompactionOrdinalMap::new([0, 1, 2]);
        let mapped = map_compaction_parts(
            &mut ordinals,
            CompactionStrategy::DeterministicTrim,
            0,
            None,
            None,
        )
        .expect("no-op maps");
        let run_id = fixture.run.id.clone();
        let error = persist_context_compaction_with(
            &hub,
            &fixture.run.id,
            RunProgress::default(),
            mapped,
            move |_run_id, _input| async move {
                Err(CompactionCommitFailure::Storage(
                    chat2db_storage::StorageError::OutcomeUnknown {
                        operation: "compact agent run",
                        id: run_id,
                    },
                ))
            },
        )
        .await
        .expect_err("unknown outcome fails");
        assert_eq!(error.api_error().code, "storage_outcome_unknown");
        assert!(hub.cached_snapshot(&fixture.run.id).await.is_err());
        assert!(subscription.next_event().await.is_err());
    }

    #[tokio::test]
    async fn post_commit_projection_failure_invalidates_the_hub() {
        let fixture = setup();
        let hub = AgentRunHub::new();
        register(&hub, &fixture.run).await;
        let (_ordinals, mapped) = summary_mapping();
        let storage = fixture.storage.clone();
        let error = persist_context_compaction_with(
            &hub,
            &fixture.run.id,
            RunProgress::default(),
            mapped,
            move |run_id, input| async move {
                let mut compacted = storage
                    .compact_agent_run(&run_id, StorageRunStatus::Running, input)
                    .map_err(CompactionCommitFailure::Storage)?;
                compacted.run.started_at_ms = None;
                Ok(compacted)
            },
        )
        .await
        .expect_err("projection failure is indeterminate");
        assert_eq!(error.kind(), AppErrorKind::Internal);
        assert!(hub.cached_snapshot(&fixture.run.id).await.is_err());
        assert_eq!(
            fixture
                .storage
                .get_agent_run(&fixture.run.id)
                .expect("durable run reloads")
                .expect("durable run exists")
                .last_sequence,
            2
        );
    }

    #[tokio::test]
    async fn indeterminate_commit_executor_failure_invalidates_the_hub() {
        let fixture = setup();
        let hub = AgentRunHub::new();
        register(&hub, &fixture.run).await;
        let mut ordinals = CompactionOrdinalMap::new([0, 1, 2]);
        let mapped = map_compaction_parts(
            &mut ordinals,
            CompactionStrategy::DeterministicTrim,
            0,
            None,
            None,
        )
        .expect("no-op maps");
        let error = persist_context_compaction_with(
            &hub,
            &fixture.run.id,
            RunProgress::default(),
            mapped,
            |_run_id, _input| async move {
                Err(CompactionCommitFailure::Indeterminate(AppError::internal()))
            },
        )
        .await
        .expect_err("executor failure is indeterminate");
        assert_eq!(error.kind(), AppErrorKind::Internal);
        assert!(hub.cached_snapshot(&fixture.run.id).await.is_err());
    }

    #[test]
    fn compaction_runtime_debug_does_not_expose_summary_text() {
        const SENTINEL: &str = "private-summary-sentinel";
        let mut ordinals = CompactionOrdinalMap::new([0, 1, 2]);
        let mapped = map_compaction_parts(
            &mut ordinals,
            CompactionStrategy::Summary,
            1,
            Some(SENTINEL),
            Some(0..2),
        )
        .expect("summary maps");
        let values = HashMap::from([("mapped", format!("{mapped:?}"))]);
        assert!(values.values().all(|value| !value.contains(SENTINEL)));
    }
}
