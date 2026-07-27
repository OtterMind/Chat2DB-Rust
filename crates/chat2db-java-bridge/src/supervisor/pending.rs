use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use chat2db_engine_protocol::wire;
use prost::Message;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};

use crate::{DeliveryOutcome, SessionState, error::PendingFailure, state::SessionStateCell};

use super::jdbc::{QueryEvent, query_event_from_payload};

const MAX_RETIRED_REQUESTS: usize = 1024;
const MAX_CREDIT_GRANT: u32 = wire::JdbcProtocolLimit::MaxCreditGrant as u32;
const MAX_OUTSTANDING_CREDITS: u32 = wire::JdbcCreditWindowLimit::MaxOutstandingCredits as u32;
const MAX_ERROR_CAUSES: usize = wire::JdbcProtocolLimit::MaxErrorCauses as usize;
const MAX_DRIVER_ARTIFACTS: u32 = wire::JdbcProtocolLimit::MaxDriverArtifacts as u32;
const MAX_DRIVER_ID_BYTES: usize = wire::JdbcProtocolLimit::MaxDriverIdBytes as usize;
const MAX_DRIVER_CLASS_BYTES: usize = wire::JdbcProtocolLimit::MaxDriverClassBytes as usize;
const MAX_COLUMNS: usize = wire::JdbcProtocolLimit::MaxColumns as usize;
const MAX_BATCH_ROWS: usize = wire::JdbcProtocolLimit::MaxBatchRows as usize;
const MAX_SCALAR_BYTES: usize = wire::JdbcProtocolLimit::MaxScalarBytes as usize;
const MAX_BATCH_BYTES: usize = wire::JdbcProtocolLimit::MaxBatchBytes as usize;
const DEFAULT_RESULT_BYTES: u64 = wire::JdbcResultByteLimit::DefaultResultBytes as u64;
const MAX_COMMUNITY_PLUGINS: usize = wire::CommunityCountLimit::MaxPlugins as usize;
const MAX_COMMUNITY_DRIVERS: usize = wire::CommunityCountLimit::MaxDriverConfigs as usize;
const MAX_COMMUNITY_DOWNLOAD_URLS: usize =
    wire::CommunityDownloadUrlLimit::MaxDownloadUrls as usize;
const MAX_COMMUNITY_SCHEMAS: usize = wire::CommunitySchemaCountLimit::MaxSchemas as usize;
const MAX_COMMUNITY_DATABASES: usize = wire::CommunityDatabaseCountLimit::MaxDatabases as usize;
const MAX_COMMUNITY_TABLES: usize = wire::CommunityTableCountLimit::MaxTables as usize;
const MAX_COMMUNITY_VIEWS: usize = wire::CommunityViewCountLimit::MaxViews as usize;
const MAX_COMMUNITY_KEYS: usize = wire::CommunityKeyCountLimit::MaxKeys as usize;
const MAX_COMMUNITY_FUNCTIONS: usize = wire::CommunityFunctionCountLimit::MaxFunctions as usize;
const MAX_COMMUNITY_PROCEDURES: usize = wire::CommunityProcedureCountLimit::MaxProcedures as usize;
const MAX_COMMUNITY_TRIGGERS: usize = wire::CommunityTriggerCountLimit::MaxTriggers as usize;
const MAX_COMMUNITY_ROUTINE_PARAMETERS: usize =
    wire::CommunityRoutineParameterCountLimit::MaxParameters as usize;
const MAX_COMMUNITY_COLUMNS: usize = wire::CommunityColumnCountLimit::MaxColumns as usize;
const MAX_COMMUNITY_INDEXES: usize = wire::CommunityIndexCountLimit::MaxIndexes as usize;
const MAX_COMMUNITY_INDEX_COLUMNS: usize =
    wire::CommunityIndexColumnCountLimit::MaxIndexColumns as usize;
const MAX_COMMUNITY_STATEMENTS: usize = wire::CommunityCountLimit::MaxStatements as usize;
const MAX_COMMUNITY_SQL_DIAGNOSTICS: usize =
    wire::CommunitySqlDiagnosticCountLimit::MaxDiagnostics as usize;
const MAX_COMMUNITY_SQL_COMPLETION_CANDIDATES: usize =
    wire::CommunitySqlCompletionCandidateCountLimit::MaxCandidates as usize;
const MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINTS: usize =
    wire::CommunitySqlCompletionEditorHintCountLimit::MaxEditorHints as usize;
const MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS: usize =
    wire::CommunitySqlCompletionEditorHintItemCountLimit::MaxEditorHintItems as usize;
const MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS: usize =
    wire::CommunitySqlCompletionSnippetSlotCountLimit::MaxSnippetSlots as usize;
const MAX_COMMUNITY_DATABASE_TYPE_BYTES: usize =
    wire::CommunityByteLimit::MaxDatabaseTypeBytes as usize;
const MAX_COMMUNITY_PLUGIN_NAME_BYTES: usize =
    wire::CommunityByteLimit::MaxPluginNameBytes as usize;
const MAX_COMMUNITY_SOURCE_COMMIT_BYTES: usize =
    wire::CommunityByteLimit::MaxSourceCommitBytes as usize;
const MAX_COMMUNITY_COMMENT_BYTES: usize = wire::CommunityByteLimit::MaxCommentBytes as usize;
const MAX_COMMUNITY_RESPONSE_BYTES: usize = wire::CommunityByteLimit::MaxResponseBytes as usize;
const MAX_SQL_BYTES: usize = wire::JdbcProtocolLimit::MaxSqlBytes as usize;

pub(super) enum PendingSink {
    Unary {
        response: oneshot::Sender<Result<wire::ServerEnvelope, PendingFailure>>,
        session_state: Option<Arc<SessionStateCell>>,
    },
    Stream {
        events: mpsc::Sender<Result<QueryEvent, PendingFailure>>,
        event_capacity: usize,
        initial_credits: u32,
        session_state: Arc<SessionStateCell>,
        budgets: QueryBudgets,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct QueryBudgets {
    pub(super) max_rows: u64,
    pub(super) target_batch_rows: u32,
    pub(super) target_batch_bytes: u32,
    pub(super) max_result_bytes: u64,
}

impl QueryBudgets {
    pub(super) fn from_options(options: &wire::QueryOptions) -> Self {
        Self {
            max_rows: options.max_rows,
            target_batch_rows: options.target_batch_rows,
            target_batch_bytes: options.target_batch_bytes,
            max_result_bytes: if options.max_result_bytes == 0 {
                DEFAULT_RESULT_BYTES
            } else {
                options.max_result_bytes
            },
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum ControlEffect {
    GrantCredits {
        target_request_id: String,
        batch_credits: u32,
    },
    Cancel {
        target_request_id: String,
    },
}

#[derive(Clone, Debug)]
pub(super) enum PendingLane {
    Retireable,
    FatalOnUnknown,
    Stream,
    Control(ControlEffect),
}

impl PendingLane {
    pub(super) const fn consumes_normal_slot(&self) -> bool {
        !matches!(self, Self::Control(_))
    }

    const fn unknown_outcome_is_fatal(&self) -> bool {
        matches!(self, Self::FatalOnUnknown | Self::Stream | Self::Control(_))
    }
}

pub(super) struct PendingRequests {
    active: HashMap<String, PendingRequest>,
    retired: RetiredRequests,
    normal_count: usize,
    control_count: usize,
    cancelling_targets: HashSet<String>,
}

impl PendingRequests {
    pub(super) fn new() -> Self {
        Self {
            active: HashMap::new(),
            retired: RetiredRequests::default(),
            normal_count: 0,
            control_count: 0,
            cancelling_targets: HashSet::new(),
        }
    }

    pub(super) fn normal_count(&self) -> usize {
        self.normal_count
    }

    pub(super) fn control_count(&self) -> usize {
        self.control_count
    }

    pub(super) fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub(super) fn contains(&self, request_id: &str) -> bool {
        self.active.contains_key(request_id)
    }

    pub(super) fn insert(
        &mut self,
        request_id: String,
        trace_id: String,
        session_id: Option<String>,
        response: PendingSink,
        deadline: Instant,
        lane: PendingLane,
    ) -> Result<(), (PendingSink, PendingFailure)> {
        if let PendingLane::Control(effect) = &lane
            && let Err(failure) = self.prepare_control(effect, session_id.as_deref())
        {
            return Err((response, failure));
        }

        let stream_validation = validate_stream_registration(&response, session_id.as_deref());
        if let Err(failure) = stream_validation {
            return Err((response, failure));
        }

        let kind = match response {
            PendingSink::Unary {
                response,
                session_state,
            } => PendingKind::Unary {
                response,
                session_state,
            },
            PendingSink::Stream {
                events,
                event_capacity,
                initial_credits,
                session_state,
                budgets,
            } => PendingKind::Stream(StreamPending {
                events: Some(events),
                event_capacity,
                session_state,
                budgets,
                abandoned: false,
                started: false,
                column_count: 0,
                next_sequence: 0,
                next_row_offset: 0,
                result_bytes: 0,
                outstanding_credits: initial_credits,
            }),
        };
        if lane.consumes_normal_slot() {
            self.normal_count = self.normal_count.saturating_add(1);
        } else {
            self.control_count = self.control_count.saturating_add(1);
        }
        self.active.insert(
            request_id,
            PendingRequest {
                trace_id,
                session_id,
                kind,
                deadline,
                lane,
            },
        );
        Ok(())
    }

    pub(super) fn reject_with_failure(&mut self, request_id: &str, failure: PendingFailure) {
        if let Some(request) = self.remove(request_id) {
            self.rollback_control(&request.lane);
            request.fail(failure);
        }
    }

    pub(super) fn retire(&mut self, request_id: String) -> Option<String> {
        let request = self.remove(&request_id)?;
        let fatal = request.lane.unknown_outcome_is_fatal().then(|| {
            format!("request {request_id} was abandoned after delivery; engine state is unknown")
        });
        self.rollback_control(&request.lane);
        if fatal.is_none() {
            self.retired.insert(request_id, request.trace_id);
        }
        fatal
    }

    pub(super) fn abandon_stream(&mut self, request_id: &str, session_id: &str) {
        let Some(request) = self.active.get_mut(request_id) else {
            return;
        };
        if request.session_id.as_deref() != Some(session_id) {
            return;
        }
        if let PendingKind::Stream(stream) = &mut request.kind {
            stream.abandoned = true;
            stream.events = None;
        }
    }

    pub(super) fn next_deadline(&self) -> Option<Instant> {
        self.active.values().map(|request| request.deadline).min()
    }

    pub(super) fn expire(&mut self, now: Instant) -> Option<String> {
        let expired = self
            .active
            .iter()
            .filter(|(_, request)| request.deadline <= now)
            .map(|(request_id, _)| request_id.clone())
            .collect::<Vec<_>>();
        let mut fatal = None;
        for request_id in expired {
            let Some(request) = self.remove(&request_id) else {
                continue;
            };
            let fatal_on_timeout = request.lane.unknown_outcome_is_fatal();
            let trace_id = request.trace_id.clone();
            self.rollback_control(&request.lane);
            request.fail(PendingFailure::Timeout {
                request_id: request_id.clone(),
                outcome: DeliveryOutcome::Unknown,
            });
            if fatal_on_timeout {
                fatal.get_or_insert_with(|| {
                    format!(
                        "request {request_id} timed out before a terminal response; generation outcome is unknown"
                    )
                });
            } else {
                self.retired.insert(request_id, trace_id);
            }
        }
        fatal
    }

    pub(super) fn route_response(&mut self, response: wire::ServerEnvelope) -> Result<(), String> {
        let meta = response
            .meta
            .as_ref()
            .ok_or_else(|| "response metadata is missing".to_owned())?;
        if meta.request_id.is_empty() {
            return Err("response request id is empty".to_owned());
        }
        let request_id = meta.request_id.clone();
        let Some(mut request) = self.remove(&request_id) else {
            return self.retired.accept(meta, response.payload.is_some());
        };
        if meta.trace_id != request.trace_id {
            return Err(format!(
                "response trace id does not match request {}",
                meta.request_id
            ));
        }
        let Some(payload) = response.payload else {
            return Err(format!("response {} has no payload", meta.request_id));
        };

        let session_state = validate_response_payload(&payload)?;
        request.apply_session_state(session_state);

        match &mut request.kind {
            PendingKind::Unary { .. } => {
                if meta.sequence != 0 {
                    return Err(format!(
                        "unary response {} used non-zero sequence {}",
                        meta.request_id, meta.sequence
                    ));
                }
                if !meta.terminal {
                    return Err(format!(
                        "unary response {} was not terminal",
                        meta.request_id
                    ));
                }
                self.apply_control_response(&request.lane, &payload)?;
                let PendingKind::Unary {
                    response: sender, ..
                } = request.kind
                else {
                    unreachable!("the unary branch must retain a unary response sender");
                };
                let _ = sender.send(Ok(wire::ServerEnvelope {
                    meta: Some(meta.clone()),
                    payload: Some(payload),
                }));
                Ok(())
            }
            PendingKind::Stream(stream) => {
                let terminal = stream.route(meta, payload)?;
                if terminal {
                    Ok(())
                } else {
                    self.restore(request_id, request);
                    Ok(())
                }
            }
        }
    }

    pub(super) fn fail_all(&mut self, message: &str) {
        for (_, request) in self.active.drain() {
            request.fail(PendingFailure::Unavailable {
                message: message.to_owned(),
                outcome: DeliveryOutcome::Unknown,
            });
        }
        self.normal_count = 0;
        self.control_count = 0;
        self.cancelling_targets.clear();
    }

    fn prepare_control(
        &mut self,
        effect: &ControlEffect,
        session_id: Option<&str>,
    ) -> Result<(), PendingFailure> {
        let (target_request_id, grant) = match effect {
            ControlEffect::GrantCredits {
                target_request_id,
                batch_credits,
            } => (target_request_id, Some(*batch_credits)),
            ControlEffect::Cancel { target_request_id } => (target_request_id, None),
        };
        let Some(target) = self.active.get_mut(target_request_id) else {
            return Err(PendingFailure::InvalidRequest(format!(
                "target query stream {target_request_id} is not active"
            )));
        };
        if target.session_id.as_deref() != session_id {
            return Err(PendingFailure::InvalidRequest(format!(
                "control request session does not match query stream {target_request_id}"
            )));
        }
        let PendingKind::Stream(stream) = &mut target.kind else {
            return Err(PendingFailure::InvalidRequest(format!(
                "control target {target_request_id} is not a query stream"
            )));
        };
        if stream.abandoned && grant.is_some() {
            return Err(PendingFailure::InvalidRequest(format!(
                "query stream {target_request_id} has been abandoned"
            )));
        }
        if let Some(batch_credits) = grant {
            stream.reserve_credits(batch_credits)?;
        } else if !self.cancelling_targets.insert(target_request_id.clone()) {
            return Err(PendingFailure::InvalidRequest(format!(
                "query stream {target_request_id} already has a cancellation in flight"
            )));
        }
        Ok(())
    }

    fn apply_control_response(
        &mut self,
        lane: &PendingLane,
        payload: &wire::server_envelope::Payload,
    ) -> Result<(), String> {
        match lane {
            PendingLane::Control(ControlEffect::GrantCredits {
                target_request_id,
                batch_credits,
            }) => {
                let accepted = match payload {
                    wire::server_envelope::Payload::CreditsGranted(granted) => {
                        if granted.accepted_batch_credits > *batch_credits {
                            return Err(format!(
                                "credit response for {target_request_id} accepted more credits than requested"
                            ));
                        }
                        granted.accepted_batch_credits
                    }
                    wire::server_envelope::Payload::Error(_) => 0,
                    _ => {
                        return Err(format!(
                            "credit request for {target_request_id} used an unexpected response payload"
                        ));
                    }
                };
                self.release_reserved_credits(
                    target_request_id,
                    batch_credits.saturating_sub(accepted),
                );
            }
            PendingLane::Control(ControlEffect::Cancel { target_request_id }) => {
                if !matches!(
                    payload,
                    wire::server_envelope::Payload::OperationCancelled(_)
                        | wire::server_envelope::Payload::Error(_)
                ) {
                    return Err(format!(
                        "cancel request for {target_request_id} used an unexpected response payload"
                    ));
                }
                self.cancelling_targets.remove(target_request_id);
            }
            PendingLane::Retireable | PendingLane::FatalOnUnknown | PendingLane::Stream => {}
        }
        Ok(())
    }

    fn rollback_control(&mut self, lane: &PendingLane) {
        match lane {
            PendingLane::Control(ControlEffect::GrantCredits {
                target_request_id,
                batch_credits,
            }) => self.release_reserved_credits(target_request_id, *batch_credits),
            PendingLane::Control(ControlEffect::Cancel { target_request_id }) => {
                self.cancelling_targets.remove(target_request_id);
            }
            PendingLane::Retireable | PendingLane::FatalOnUnknown | PendingLane::Stream => {}
        }
    }

    fn release_reserved_credits(&mut self, target_request_id: &str, credits: u32) {
        let Some(PendingRequest {
            kind: PendingKind::Stream(stream),
            ..
        }) = self.active.get_mut(target_request_id)
        else {
            return;
        };
        stream.outstanding_credits = stream.outstanding_credits.saturating_sub(credits);
    }

    fn remove(&mut self, request_id: &str) -> Option<PendingRequest> {
        let request = self.active.remove(request_id)?;
        if request.lane.consumes_normal_slot() {
            self.normal_count = self.normal_count.saturating_sub(1);
        } else {
            self.control_count = self.control_count.saturating_sub(1);
        }
        Some(request)
    }

    fn restore(&mut self, request_id: String, request: PendingRequest) {
        if request.lane.consumes_normal_slot() {
            self.normal_count = self.normal_count.saturating_add(1);
        } else {
            self.control_count = self.control_count.saturating_add(1);
        }
        self.active.insert(request_id, request);
    }
}

struct PendingRequest {
    trace_id: String,
    session_id: Option<String>,
    kind: PendingKind,
    deadline: Instant,
    lane: PendingLane,
}

impl PendingRequest {
    fn apply_session_state(&self, state: Option<SessionState>) {
        let Some(state) = state else {
            return;
        };
        match &self.kind {
            PendingKind::Unary {
                session_state: Some(cell),
                ..
            } => cell.set(state),
            PendingKind::Stream(stream) => stream.session_state.set(state),
            PendingKind::Unary {
                session_state: None,
                ..
            } => {}
        }
    }

    fn fail(self, failure: PendingFailure) {
        match self.kind {
            PendingKind::Unary { response, .. } => {
                let _ = response.send(Err(failure));
            }
            PendingKind::Stream(stream) => stream.fail(failure),
        }
    }
}

enum PendingKind {
    Unary {
        response: oneshot::Sender<Result<wire::ServerEnvelope, PendingFailure>>,
        session_state: Option<Arc<SessionStateCell>>,
    },
    Stream(StreamPending),
}

struct StreamPending {
    events: Option<mpsc::Sender<Result<QueryEvent, PendingFailure>>>,
    event_capacity: usize,
    session_state: Arc<SessionStateCell>,
    budgets: QueryBudgets,
    abandoned: bool,
    started: bool,
    column_count: usize,
    next_sequence: u64,
    next_row_offset: u64,
    result_bytes: u64,
    outstanding_credits: u32,
}

impl StreamPending {
    fn reserve_credits(&mut self, batch_credits: u32) -> Result<(), PendingFailure> {
        if batch_credits == 0 {
            return Err(PendingFailure::InvalidRequest(
                "credit grants must be greater than zero".to_owned(),
            ));
        }
        if batch_credits > MAX_CREDIT_GRANT {
            return Err(PendingFailure::InvalidRequest(format!(
                "one credit grant cannot exceed {MAX_CREDIT_GRANT}"
            )));
        }
        let Some(events) = &self.events else {
            return Err(PendingFailure::InvalidRequest(
                "cannot grant credits to an abandoned query stream".to_owned(),
            ));
        };
        let reserved_non_batch = usize::from(!self.started) + 1;
        let available = events
            .capacity()
            .saturating_sub(reserved_non_batch)
            .saturating_sub(usize::try_from(self.outstanding_credits).unwrap_or(usize::MAX));
        if usize::try_from(batch_credits).unwrap_or(usize::MAX) > available {
            return Err(PendingFailure::InvalidRequest(format!(
                "credit grant {batch_credits} exceeds bounded stream capacity {available}"
            )));
        }
        let Some(outstanding) = self.outstanding_credits.checked_add(batch_credits) else {
            return Err(PendingFailure::InvalidRequest(
                "query credit count overflowed".to_owned(),
            ));
        };
        if outstanding > MAX_OUTSTANDING_CREDITS {
            return Err(PendingFailure::InvalidRequest(format!(
                "outstanding query credits cannot exceed {MAX_OUTSTANDING_CREDITS}"
            )));
        }
        self.outstanding_credits = outstanding;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn route(
        &mut self,
        meta: &wire::ResponseMeta,
        payload: wire::server_envelope::Payload,
    ) -> Result<bool, String> {
        if meta.sequence != self.next_sequence {
            return Err(format!(
                "query stream {} expected sequence {} but received {}",
                meta.request_id, self.next_sequence, meta.sequence
            ));
        }
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| format!("query stream {} sequence overflowed", meta.request_id))?;

        match payload {
            wire::server_envelope::Payload::QueryStarted(started) => {
                if meta.terminal || self.started || meta.sequence != 0 {
                    return Err(format!(
                        "query stream {} used an invalid query-started envelope",
                        meta.request_id
                    ));
                }
                if started.columns.len() > MAX_COLUMNS {
                    return Err(format!(
                        "query stream {} exceeded the {MAX_COLUMNS}-column limit",
                        meta.request_id
                    ));
                }
                for (index, column) in started.columns.iter().enumerate() {
                    let expected = u32::try_from(index + 1).unwrap_or(u32::MAX);
                    if column.ordinal != expected {
                        return Err(format!(
                            "query stream {} returned non-contiguous column ordinals",
                            meta.request_id
                        ));
                    }
                }
                self.started = true;
                self.column_count = started.columns.len();
                self.emit(query_event_from_payload(
                    wire::server_envelope::Payload::QueryStarted(started),
                )?)?;
                Ok(false)
            }
            wire::server_envelope::Payload::RowBatch(batch) => {
                if meta.terminal || !self.started {
                    return Err(format!(
                        "query stream {} used an invalid row-batch envelope",
                        meta.request_id
                    ));
                }
                if self.outstanding_credits == 0 {
                    return Err(format!(
                        "query stream {} emitted a row batch without credit",
                        meta.request_id
                    ));
                }
                if batch.rows.len() > MAX_BATCH_ROWS {
                    return Err(format!(
                        "query stream {} exceeded the {MAX_BATCH_ROWS}-row batch limit",
                        meta.request_id
                    ));
                }
                if self.budgets.target_batch_rows != 0
                    && batch.rows.len()
                        > usize::try_from(self.budgets.target_batch_rows).unwrap_or(usize::MAX)
                {
                    return Err(format!(
                        "query stream {} exceeded its target_batch_rows budget",
                        meta.request_id
                    ));
                }
                let batch_bytes = batch.encoded_len();
                if batch_bytes > MAX_BATCH_BYTES {
                    return Err(format!(
                        "query stream {} exceeded the {MAX_BATCH_BYTES}-byte batch limit",
                        meta.request_id
                    ));
                }
                if self.budgets.target_batch_bytes != 0
                    && batch_bytes
                        > usize::try_from(self.budgets.target_batch_bytes).unwrap_or(usize::MAX)
                {
                    return Err(format!(
                        "query stream {} exceeded its target_batch_bytes budget",
                        meta.request_id
                    ));
                }
                if batch.start_row_offset != self.next_row_offset {
                    return Err(format!(
                        "query stream {} returned a non-contiguous row offset",
                        meta.request_id
                    ));
                }
                if batch
                    .rows
                    .iter()
                    .any(|row| row.values.len() != self.column_count)
                {
                    return Err(format!(
                        "query stream {} returned a row with the wrong column count",
                        meta.request_id
                    ));
                }
                let row_count = u64::try_from(batch.rows.len()).unwrap_or(u64::MAX);
                let next_row_offset =
                    self.next_row_offset.checked_add(row_count).ok_or_else(|| {
                        format!("query stream {} row count overflowed", meta.request_id)
                    })?;
                if self.budgets.max_rows != 0 && next_row_offset > self.budgets.max_rows {
                    return Err(format!(
                        "query stream {} exceeded its max_rows budget",
                        meta.request_id
                    ));
                }
                let batch_result_bytes = batch.rows.iter().try_fold(0_u64, |total, row| {
                    total
                        .checked_add(u64::try_from(row.encoded_len()).unwrap_or(u64::MAX))
                        .ok_or_else(|| {
                            format!(
                                "query stream {} result byte count overflowed",
                                meta.request_id
                            )
                        })
                })?;
                let result_bytes = self
                    .result_bytes
                    .checked_add(batch_result_bytes)
                    .ok_or_else(|| {
                        format!(
                            "query stream {} result byte count overflowed",
                            meta.request_id
                        )
                    })?;
                if result_bytes > self.budgets.max_result_bytes {
                    return Err(format!(
                        "query stream {} exceeded its max_result_bytes budget",
                        meta.request_id
                    ));
                }
                self.outstanding_credits = self.outstanding_credits.saturating_sub(1);
                self.next_row_offset = next_row_offset;
                self.result_bytes = result_bytes;
                self.emit(query_event_from_payload(
                    wire::server_envelope::Payload::RowBatch(batch),
                )?)?;
                Ok(false)
            }
            wire::server_envelope::Payload::QueryCompleted(completed) => {
                if !meta.terminal || !self.started || completed.row_count != self.next_row_offset {
                    return Err(format!(
                        "query stream {} used an invalid query-completed envelope",
                        meta.request_id
                    ));
                }
                if completed.truncated_by_max_rows
                    && (self.budgets.max_rows == 0 || completed.row_count != self.budgets.max_rows)
                {
                    return Err(format!(
                        "query stream {} reported inconsistent max_rows truncation",
                        meta.request_id
                    ));
                }
                self.emit(query_event_from_payload(
                    wire::server_envelope::Payload::QueryCompleted(completed),
                )?)?;
                Ok(true)
            }
            wire::server_envelope::Payload::Error(error) => {
                if !meta.terminal {
                    return Err(format!(
                        "query stream {} returned a non-terminal error",
                        meta.request_id
                    ));
                }
                self.emit_failure(PendingFailure::Remote(Box::new(error)));
                Ok(true)
            }
            _ => Err(format!(
                "query stream {} used an unexpected response payload",
                meta.request_id
            )),
        }
    }

    fn emit(&mut self, event: QueryEvent) -> Result<(), String> {
        let Some(events) = &self.events else {
            return Ok(());
        };
        match events.try_send(Ok(event)) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.abandoned = true;
                self.events = None;
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(format!(
                "query stream event channel exceeded its bounded capacity {}",
                self.event_capacity
            )),
        }
    }

    fn emit_failure(&mut self, failure: PendingFailure) {
        let Some(events) = &self.events else {
            return;
        };
        let _ = events.try_send(Err(failure));
    }

    fn fail(mut self, failure: PendingFailure) {
        self.emit_failure(failure);
    }
}

#[allow(clippy::too_many_lines)]
fn validate_response_payload(
    payload: &wire::server_envelope::Payload,
) -> Result<Option<SessionState>, String> {
    match payload {
        wire::server_envelope::Payload::Error(error) => validate_engine_error(error),
        wire::server_envelope::Payload::DriverLoaded(driver) => {
            validate_non_empty_bytes(&driver.driver_id, MAX_DRIVER_ID_BYTES, "driver id")?;
            validate_non_empty_bytes(&driver.driver_class, MAX_DRIVER_CLASS_BYTES, "driver class")?;
            if driver.artifact_count > MAX_DRIVER_ARTIFACTS {
                return Err(format!(
                    "driver response exceeded the {MAX_DRIVER_ARTIFACTS}-artifact limit"
                ));
            }
            Ok(None)
        }
        wire::server_envelope::Payload::DriverUnloaded(driver) => {
            validate_non_empty_bytes(&driver.driver_id, MAX_DRIVER_ID_BYTES, "driver id")?;
            Ok(None)
        }
        wire::server_envelope::Payload::SessionOpened(opened) => {
            validate_non_empty_bytes(&opened.session_id, MAX_DRIVER_ID_BYTES, "session id")?;
            if let Some(database) = &opened.database {
                validate_scalar(&database.name, "database name")?;
                validate_scalar(&database.version, "database version")?;
                validate_scalar(&database.driver_name, "database driver name")?;
                validate_scalar(&database.driver_version, "database driver version")?;
            }
            SessionState::from_wire(opened.session_state).map(Some)
        }
        wire::server_envelope::Payload::SessionClosed(closed) => {
            SessionState::from_wire(closed.session_state).map(Some)
        }
        wire::server_envelope::Payload::TransactionStarted(started) => {
            validate_non_empty_bytes(
                &started.transaction_id,
                MAX_DRIVER_ID_BYTES,
                "transaction id",
            )?;
            SessionState::from_wire(started.session_state).map(Some)
        }
        wire::server_envelope::Payload::TransactionCommitted(committed) => {
            validate_non_empty_bytes(
                &committed.transaction_id,
                MAX_DRIVER_ID_BYTES,
                "transaction id",
            )?;
            SessionState::from_wire(committed.session_state).map(Some)
        }
        wire::server_envelope::Payload::TransactionRolledBack(rolled_back) => {
            validate_non_empty_bytes(
                &rolled_back.transaction_id,
                MAX_DRIVER_ID_BYTES,
                "transaction id",
            )?;
            SessionState::from_wire(rolled_back.session_state).map(Some)
        }
        wire::server_envelope::Payload::QueryStarted(started) => {
            for column in &started.columns {
                validate_scalar(&column.label, "column label")?;
                validate_scalar(&column.name, "column name")?;
                validate_scalar(&column.jdbc_type_name, "column JDBC type name")?;
                validate_optional_scalar(column.catalog_name.as_deref(), "column catalog")?;
                validate_optional_scalar(column.schema_name.as_deref(), "column schema")?;
                validate_optional_scalar(column.table_name.as_deref(), "column table")?;
            }
            Ok(None)
        }
        wire::server_envelope::Payload::RowBatch(batch) => {
            for row in &batch.rows {
                for value in &row.values {
                    validate_jdbc_value(value)?;
                }
            }
            Ok(None)
        }
        wire::server_envelope::Payload::OperationCancelled(cancelled) => {
            match wire::CancelDisposition::try_from(cancelled.disposition) {
                Ok(wire::CancelDisposition::Unspecified) | Err(_) => Err(format!(
                    "operation-cancelled response used unknown disposition {}",
                    cancelled.disposition
                )),
                Ok(_) => Ok(None),
            }
        }
        wire::server_envelope::Payload::CommunityPluginCatalog(catalog) => {
            validate_community_source_commit(&catalog.source_commit)?;
            let mut field_bytes = 0;
            add_community_response_field(&mut field_bytes, &catalog.source_commit)?;
            if catalog.plugins.len() > MAX_COMMUNITY_PLUGINS {
                return Err(format!(
                    "Community catalog exceeded the {MAX_COMMUNITY_PLUGINS}-plugin limit"
                ));
            }
            for plugin in &catalog.plugins {
                validate_non_empty_bytes(
                    &plugin.database_type,
                    MAX_COMMUNITY_DATABASE_TYPE_BYTES,
                    "Community database type",
                )?;
                add_community_response_field(&mut field_bytes, &plugin.database_type)?;
                validate_non_empty_bytes(
                    &plugin.name,
                    MAX_COMMUNITY_PLUGIN_NAME_BYTES,
                    "Community plugin name",
                )?;
                add_community_response_field(&mut field_bytes, &plugin.name)?;
                if plugin.drivers.len() > MAX_COMMUNITY_DRIVERS {
                    return Err(format!(
                        "Community plugin exceeded the {MAX_COMMUNITY_DRIVERS}-driver limit"
                    ));
                }
                for driver in &plugin.drivers {
                    validate_scalar(&driver.url, "Community driver URL")?;
                    validate_scalar(&driver.jdbc_driver, "Community JDBC driver")?;
                    validate_scalar(&driver.jdbc_driver_class, "Community JDBC driver class")?;
                    add_community_response_field(&mut field_bytes, &driver.url)?;
                    add_community_response_field(&mut field_bytes, &driver.jdbc_driver)?;
                    add_community_response_field(&mut field_bytes, &driver.jdbc_driver_class)?;
                    if driver.download_urls.len() > MAX_COMMUNITY_DOWNLOAD_URLS {
                        return Err(format!(
                            "Community driver exceeded the {MAX_COMMUNITY_DOWNLOAD_URLS}-URL limit"
                        ));
                    }
                    for url in &driver.download_urls {
                        validate_scalar(url, "Community driver download URL")?;
                        add_community_response_field(&mut field_bytes, url)?;
                    }
                }
            }
            validate_community_response_encoded_len(catalog)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunitySchemaList(schemas) => {
            let mut field_bytes = 0;
            if schemas.schemas.len() > MAX_COMMUNITY_SCHEMAS {
                return Err(format!(
                    "Community metadata exceeded the {MAX_COMMUNITY_SCHEMAS}-schema limit"
                ));
            }
            for schema in &schemas.schemas {
                validate_scalar(&schema.database_name, "Community schema database")?;
                validate_scalar(&schema.name, "Community schema name")?;
                validate_bytes_limit(
                    &schema.comment,
                    MAX_COMMUNITY_COMMENT_BYTES,
                    "Community schema comment",
                )?;
                validate_scalar(&schema.owner, "Community schema owner")?;
                add_community_response_field(&mut field_bytes, &schema.database_name)?;
                add_community_response_field(&mut field_bytes, &schema.name)?;
                add_community_response_field(&mut field_bytes, &schema.comment)?;
                add_community_response_field(&mut field_bytes, &schema.owner)?;
            }
            validate_community_response_encoded_len(schemas)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityDatabaseList(databases) => {
            validate_community_database_list(databases)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityTableList(tables) => {
            validate_community_table_list(tables)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityTableColumnList(columns) => {
            validate_community_table_column_list(columns)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityTableIndexList(indexes) => {
            validate_community_table_index_list(indexes)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityViewList(views) => {
            validate_community_view_list(views)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityImportedKeyList(keys)
        | wire::server_envelope::Payload::CommunityExportedKeyList(keys) => {
            validate_community_foreign_key_list(keys)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityPrimaryKeyList(keys) => {
            validate_community_primary_key_list(keys)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityFunctionList(functions) => {
            validate_community_function_list(functions)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityFunction(function) => {
            validate_community_function(function)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityFunctionParameterList(parameters) => {
            validate_community_function_parameter_list(parameters)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityProcedureList(procedures) => {
            validate_community_procedure_list(procedures)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityProcedure(procedure) => {
            validate_community_procedure(procedure)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityProcedureParameterList(parameters) => {
            validate_community_procedure_parameter_list(parameters)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityTriggerList(triggers) => {
            validate_community_trigger_list(triggers)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityTrigger(trigger) => {
            validate_community_trigger(trigger)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityBuiltSql(built) => {
            validate_non_empty_bytes(&built.sql, MAX_SQL_BYTES, "Community built SQL")?;
            let mut field_bytes = 0;
            add_community_response_field(&mut field_bytes, &built.sql)?;
            validate_community_response_encoded_len(built)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityBuiltDml(built) => {
            validate_non_empty_bytes(&built.sql, MAX_SQL_BYTES, "Community built DML")?;
            let mut field_bytes = 0;
            add_community_response_field(&mut field_bytes, &built.sql)?;
            validate_community_response_encoded_len(built)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityBuiltNamespaceSql(built) => {
            validate_non_empty_bytes(&built.sql, MAX_SQL_BYTES, "Community built namespace SQL")?;
            let mut field_bytes = 0;
            add_community_response_field(&mut field_bytes, &built.sql)?;
            validate_community_response_encoded_len(built)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunitySqlAnalysis(analysis) => {
            let mut field_bytes = 0;
            validate_community_parsed_statements(&analysis.statements, &mut field_bytes)?;
            validate_community_response_encoded_len(analysis)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunitySqlValidation(validation) => {
            let mut field_bytes = 0;
            validate_community_parsed_statements(&validation.statements, &mut field_bytes)?;
            if validation.diagnostics.len() > MAX_COMMUNITY_SQL_DIAGNOSTICS {
                return Err(format!(
                    "Community SQL validation exceeded the {MAX_COMMUNITY_SQL_DIAGNOSTICS}-diagnostic limit"
                ));
            }
            for diagnostic in &validation.diagnostics {
                validate_bytes_limit(
                    &diagnostic.token_text,
                    MAX_SQL_BYTES,
                    "Community SQL diagnostic token text",
                )?;
                validate_bytes_limit(
                    &diagnostic.message,
                    MAX_COMMUNITY_COMMENT_BYTES,
                    "Community SQL diagnostic message",
                )?;
                add_community_response_field(&mut field_bytes, &diagnostic.token_text)?;
                add_community_response_field(&mut field_bytes, &diagnostic.message)?;
            }
            if validation.valid != validation.diagnostics.is_empty() {
                return Err(
                    "Community SQL validation validity disagreed with its diagnostics".to_owned(),
                );
            }
            validate_community_response_encoded_len(validation)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunityFormattedSql(formatted) => {
            validate_non_empty_bytes(&formatted.sql, MAX_SQL_BYTES, "Community formatted SQL")?;
            let mut field_bytes = 0;
            add_community_response_field(&mut field_bytes, &formatted.sql)?;
            validate_community_response_encoded_len(formatted)?;
            Ok(None)
        }
        wire::server_envelope::Payload::CommunitySqlCompletion(completion) => {
            validate_community_sql_completion(completion)?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn validate_community_sql_completion(
    completion: &wire::CommunitySqlCompletion,
) -> Result<(), String> {
    validate_completion_status(&completion.status)?;
    validate_offset_range(
        completion.replace_start_utf16,
        completion.replace_end_utf16,
        "Community SQL-completion replacement",
    )?;
    if completion.candidates.len() > MAX_COMMUNITY_SQL_COMPLETION_CANDIDATES {
        return Err(format!(
            "Community SQL completion exceeded the {MAX_COMMUNITY_SQL_COMPLETION_CANDIDATES}-candidate limit"
        ));
    }
    if completion.editor_hints.len() > MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINTS {
        return Err(format!(
            "Community SQL completion exceeded the {MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINTS}-editor-hint limit"
        ));
    }

    let mut field_bytes = 0_usize;
    add_community_response_field(&mut field_bytes, &completion.status)?;
    validate_optional_scalar(
        completion.reason_code.as_deref(),
        "Community SQL-completion reason code",
    )?;
    add_optional_community_response_field(&mut field_bytes, completion.reason_code.as_deref())?;
    let mut snippet_slots = 0_usize;
    for candidate in &completion.candidates {
        validate_completion_candidate(candidate, &mut field_bytes, &mut snippet_slots)?;
    }
    let mut hint_items = 0_usize;
    for hint in &completion.editor_hints {
        validate_completion_editor_hint(hint, &mut field_bytes, &mut hint_items)?;
    }
    validate_community_response_encoded_len(completion)
}

fn validate_completion_candidate(
    candidate: &wire::CommunitySqlCompletionCandidate,
    field_bytes: &mut usize,
    snippet_slots: &mut usize,
) -> Result<(), String> {
    validate_non_empty_bytes(
        &candidate.label,
        MAX_SCALAR_BYTES,
        "Community SQL-completion candidate label",
    )?;
    validate_completion_candidate_type(&candidate.r#type)?;
    validate_completion_insert_type(&candidate.insert_type)?;
    match (candidate.replace_start_utf16, candidate.replace_end_utf16) {
        (Some(start), Some(end)) => {
            validate_offset_range(start, end, "Community SQL-completion candidate replacement")?;
        }
        (None, None) => {}
        _ => {
            return Err(
                "Community SQL-completion candidate provided only one replacement endpoint"
                    .to_owned(),
            );
        }
    }
    if let Some(mode) = candidate.parameter_mode.as_deref() {
        validate_completion_parameter_mode(mode)?;
    }

    for (value, field) in [
        (candidate.id.as_deref(), "candidate id"),
        (candidate.insert_text.as_deref(), "candidate insert text"),
        (candidate.detail.as_deref(), "candidate detail"),
        (candidate.description.as_deref(), "candidate description"),
        (candidate.data_type.as_deref(), "candidate data type"),
        (candidate.object_type.as_deref(), "candidate object type"),
        (
            candidate.datasource_name.as_deref(),
            "candidate datasource name",
        ),
        (
            candidate.database_name.as_deref(),
            "candidate database name",
        ),
        (candidate.schema_name.as_deref(), "candidate schema name"),
        (candidate.table_name.as_deref(), "candidate table name"),
        (candidate.table_alias.as_deref(), "candidate table alias"),
        (candidate.column_name.as_deref(), "candidate column name"),
        (candidate.object_name.as_deref(), "candidate object name"),
        (candidate.sort_text.as_deref(), "candidate sort text"),
    ] {
        validate_optional_scalar(value, &format!("Community SQL-completion {field}"))?;
        add_optional_community_response_field(field_bytes, value)?;
    }
    if let Some(comment) = candidate.comment.as_deref() {
        validate_bytes_limit(
            comment,
            MAX_COMMUNITY_COMMENT_BYTES,
            "Community SQL-completion candidate comment",
        )?;
        add_community_response_field(field_bytes, comment)?;
    }
    add_community_response_field(field_bytes, &candidate.label)?;
    add_community_response_field(field_bytes, &candidate.r#type)?;
    add_community_response_field(field_bytes, &candidate.insert_type)?;
    add_optional_community_response_field(field_bytes, candidate.parameter_mode.as_deref())?;

    *snippet_slots = snippet_slots
        .checked_add(candidate.snippet_slots.len())
        .ok_or_else(|| "Community SQL-completion snippet-slot count overflowed".to_owned())?;
    if *snippet_slots > MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS {
        return Err(format!(
            "Community SQL completion exceeded the {MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS}-snippet-slot limit"
        ));
    }
    for slot in &candidate.snippet_slots {
        validate_completion_snippet_slot_type(slot)?;
        add_community_response_field(field_bytes, slot)?;
    }
    Ok(())
}

fn validate_completion_editor_hint(
    hint: &wire::CommunitySqlCompletionEditorHint,
    field_bytes: &mut usize,
    hint_items: &mut usize,
) -> Result<(), String> {
    validate_completion_editor_hint_type(&hint.r#type)?;
    add_community_response_field(field_bytes, &hint.r#type)?;
    for (label, range) in [
        ("statement", hint.statement_range.as_ref()),
        ("row", hint.row_range.as_ref()),
        ("value", hint.value_range.as_ref()),
    ] {
        if let Some(range) = range {
            validate_completion_range(range, label)?;
        }
    }

    *hint_items = hint_items
        .checked_add(hint.items.len())
        .ok_or_else(|| "Community SQL-completion editor-hint item count overflowed".to_owned())?;
    if *hint_items > MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS {
        return Err(format!(
            "Community SQL completion exceeded the {MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS}-editor-hint-item limit"
        ));
    }
    for item in &hint.items {
        for (value, field) in [
            (item.field_name.as_deref(), "field name"),
            (item.field_type.as_deref(), "field type"),
            (item.label.as_deref(), "label"),
        ] {
            validate_optional_scalar(
                value,
                &format!("Community SQL-completion editor-hint item {field}"),
            )?;
            add_optional_community_response_field(field_bytes, value)?;
        }
        if let Some(range) = item.range.as_ref() {
            validate_completion_range(range, "item")?;
        }
    }
    Ok(())
}

fn validate_offset_range(start: u32, end: u32, field: &str) -> Result<(), String> {
    if start > end {
        return Err(format!("{field} start exceeds its end"));
    }
    Ok(())
}

fn validate_completion_range(
    range: &wire::CommunitySqlCompletionRange,
    label: &str,
) -> Result<(), String> {
    if range.start_line_number == 0
        || range.start_column == 0
        || range.end_line_number == 0
        || range.end_column == 0
    {
        return Err(format!(
            "Community SQL-completion {label} range must use one-based lines and columns"
        ));
    }
    if (range.start_line_number, range.start_column) > (range.end_line_number, range.end_column) {
        return Err(format!(
            "Community SQL-completion {label} range start exceeds its end"
        ));
    }
    Ok(())
}

fn validate_completion_status(value: &str) -> Result<(), String> {
    validate_completion_value(
        value,
        &["SUCCESS", "EMPTY", "REJECTED", "UNSUPPORTED", "ERROR"],
        "status",
    )
}

fn validate_completion_insert_type(value: &str) -> Result<(), String> {
    validate_completion_value(value, &["PLAIN_TEXT", "SNIPPET"], "candidate insert type")
}

fn validate_completion_parameter_mode(value: &str) -> Result<(), String> {
    validate_completion_value(
        value,
        &["UNKNOWN", "IN", "OUT", "INOUT", "RETURN", "RESULT"],
        "candidate parameter mode",
    )
}

fn validate_completion_snippet_slot_type(value: &str) -> Result<(), String> {
    validate_completion_value(
        value,
        &["SELECT_FUNCTION", "CALL_PROCEDURE", "INSERT_COLUMN_LIST"],
        "candidate snippet slot",
    )
}

fn validate_completion_editor_hint_type(value: &str) -> Result<(), String> {
    validate_completion_value(
        value,
        &["INSERT_VALUE", "ROUTINE_PARAMETER"],
        "editor-hint type",
    )
}

fn validate_completion_candidate_type(value: &str) -> Result<(), String> {
    validate_completion_value(
        value,
        &[
            "CATALOG",
            "DATABASE",
            "SCHEMA",
            "KEYWORD",
            "TABLE",
            "VIEW",
            "TABLE_VIEW",
            "COLUMN",
            "ALL_COLUMN",
            "JOIN_CLAUSE",
            "INDEX",
            "PROCEDURE",
            "FUNCTION",
            "EVENT",
            "PARAMETER",
            "TYPE",
            "USER",
            "ROLE",
            "TABLESPACE",
            "TRIGGER",
            "SEQUENCE",
            "MATERIALIZED_VIEW",
            "PACKAGE",
            "CONSTRAINT",
            "SYNONYM",
            "ALIAS",
            "VARIABLE",
            "DBLINK",
            "ROUTINE",
            "SNIPPET",
            "TEMP_TABLE",
            "OTHER",
        ],
        "candidate type",
    )
}

fn validate_completion_value(value: &str, allowed: &[&str], field: &str) -> Result<(), String> {
    validate_non_empty_bytes(
        value,
        MAX_SCALAR_BYTES,
        &format!("Community SQL-completion {field}"),
    )?;
    if !allowed.contains(&value) {
        return Err(format!(
            "Community SQL-completion {field} used unknown value {value}"
        ));
    }
    Ok(())
}

fn validate_community_parsed_statements(
    statements: &[wire::CommunityParsedStatement],
    field_bytes: &mut usize,
) -> Result<(), String> {
    if statements.len() > MAX_COMMUNITY_STATEMENTS {
        return Err(format!(
            "Community parser exceeded the {MAX_COMMUNITY_STATEMENTS}-statement limit"
        ));
    }
    for statement in statements {
        validate_bytes_limit(&statement.sql, MAX_SQL_BYTES, "Community parsed SQL")?;
        validate_scalar(&statement.r#type, "Community parsed statement type")?;
        validate_scalar(
            &statement.statement_type,
            "Community parsed statement category",
        )?;
        add_community_response_field(field_bytes, &statement.sql)?;
        add_community_response_field(field_bytes, &statement.r#type)?;
        add_community_response_field(field_bytes, &statement.statement_type)?;
    }
    Ok(())
}

fn validate_community_database_list(databases: &wire::CommunityDatabaseList) -> Result<(), String> {
    if databases.databases.len() > MAX_COMMUNITY_DATABASES {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_DATABASES}-database limit"
        ));
    }
    let mut field_bytes = 0;
    for database in &databases.databases {
        validate_community_response_field(
            &mut field_bytes,
            &database.name,
            MAX_SCALAR_BYTES,
            "Community database name",
        )?;
        validate_community_response_field(
            &mut field_bytes,
            &database.comment,
            MAX_COMMUNITY_COMMENT_BYTES,
            "Community database comment",
        )?;
        validate_community_response_field(
            &mut field_bytes,
            &database.charset,
            MAX_SCALAR_BYTES,
            "Community database charset",
        )?;
        validate_community_response_field(
            &mut field_bytes,
            &database.collation,
            MAX_SCALAR_BYTES,
            "Community database collation",
        )?;
        validate_community_response_field(
            &mut field_bytes,
            &database.owner,
            MAX_SCALAR_BYTES,
            "Community database owner",
        )?;
    }
    validate_community_response_encoded_len(databases)
}

fn validate_community_table_list(tables: &wire::CommunityTableList) -> Result<(), String> {
    validate_community_table_values(&tables.tables, MAX_COMMUNITY_TABLES, "table")?;
    validate_community_response_encoded_len(tables)
}

fn validate_community_view_list(views: &wire::CommunityViewList) -> Result<(), String> {
    validate_community_table_values(&views.views, MAX_COMMUNITY_VIEWS, "view")?;
    validate_community_response_encoded_len(views)
}

fn validate_community_table_values(
    tables: &[wire::CommunityTable],
    maximum: usize,
    label: &str,
) -> Result<(), String> {
    if tables.len() > maximum {
        return Err(format!(
            "Community metadata exceeded the {maximum}-{label} limit"
        ));
    }
    let mut field_bytes = 0;
    for table in tables {
        for (value, field) in [
            (&table.database_name, "Community table database"),
            (&table.schema_name, "Community table schema"),
            (&table.name, "Community table name"),
            (&table.r#type, "Community table type"),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
        validate_community_response_field(
            &mut field_bytes,
            &table.comment,
            MAX_COMMUNITY_COMMENT_BYTES,
            "Community table comment",
        )?;
        validate_community_response_field(
            &mut field_bytes,
            &table.database_type,
            MAX_COMMUNITY_DATABASE_TYPE_BYTES,
            "Community table database type",
        )?;
        validate_community_response_field(
            &mut field_bytes,
            &table.ddl,
            MAX_SQL_BYTES,
            "Community table DDL",
        )?;
        for (value, field) in [
            (&table.engine, "Community table engine"),
            (&table.charset, "Community table charset"),
            (&table.collation, "Community table collation"),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
        validate_community_response_field(
            &mut field_bytes,
            &table.partition,
            MAX_SQL_BYTES,
            "Community table partition",
        )?;
        for (value, field) in [
            (&table.tablespace, "Community table tablespace"),
            (&table.create_time, "Community table create time"),
            (&table.update_time, "Community table update time"),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
    }
    Ok(())
}

fn validate_community_foreign_key_list(keys: &wire::CommunityForeignKeyList) -> Result<(), String> {
    if keys.keys.len() > MAX_COMMUNITY_KEYS {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_KEYS}-foreign-key limit"
        ));
    }
    let mut field_bytes = 0;
    for key in &keys.keys {
        for (value, field) in [
            (
                &key.primary_table_database,
                "Community foreign-key primary database",
            ),
            (
                &key.primary_table_schema,
                "Community foreign-key primary schema",
            ),
            (
                &key.primary_table_name,
                "Community foreign-key primary table",
            ),
            (
                &key.primary_column_name,
                "Community foreign-key primary column",
            ),
            (
                &key.foreign_table_database,
                "Community foreign-key foreign database",
            ),
            (
                &key.foreign_table_schema,
                "Community foreign-key foreign schema",
            ),
            (
                &key.foreign_table_name,
                "Community foreign-key foreign table",
            ),
            (
                &key.foreign_column_name,
                "Community foreign-key foreign column",
            ),
            (&key.foreign_key_name, "Community foreign-key name"),
            (
                &key.primary_key_name,
                "Community foreign-key primary-key name",
            ),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
    }
    validate_community_response_encoded_len(keys)
}

fn validate_community_primary_key_list(keys: &wire::CommunityPrimaryKeyList) -> Result<(), String> {
    if keys.keys.len() > MAX_COMMUNITY_KEYS {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_KEYS}-primary-key limit"
        ));
    }
    let mut field_bytes = 0;
    for key in &keys.keys {
        for (value, field) in [
            (&key.database_name, "Community primary-key database"),
            (&key.schema_name, "Community primary-key schema"),
            (&key.table_name, "Community primary-key table"),
            (&key.column_name, "Community primary-key column"),
            (&key.name, "Community primary-key name"),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
    }
    validate_community_response_encoded_len(keys)
}

fn validate_community_function_list(functions: &wire::CommunityFunctionList) -> Result<(), String> {
    if functions.functions.len() > MAX_COMMUNITY_FUNCTIONS {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_FUNCTIONS}-function limit"
        ));
    }
    let mut field_bytes = 0;
    for function in &functions.functions {
        validate_community_function_value(function, &mut field_bytes)?;
    }
    validate_community_response_encoded_len(functions)
}

fn validate_community_function(function: &wire::CommunityFunction) -> Result<(), String> {
    let mut field_bytes = 0;
    validate_community_function_value(function, &mut field_bytes)?;
    validate_community_response_encoded_len(function)
}

fn validate_community_function_value(
    function: &wire::CommunityFunction,
    field_bytes: &mut usize,
) -> Result<(), String> {
    for (value, field) in [
        (&function.database_name, "Community function database"),
        (&function.schema_name, "Community function schema"),
        (&function.name, "Community function name"),
        (&function.specific_name, "Community function specific name"),
    ] {
        validate_community_response_field(field_bytes, value, MAX_SCALAR_BYTES, field)?;
    }
    validate_community_response_field(
        field_bytes,
        &function.remarks,
        MAX_COMMUNITY_COMMENT_BYTES,
        "Community function remarks",
    )?;
    validate_community_response_field(
        field_bytes,
        &function.body,
        MAX_SQL_BYTES,
        "Community function body",
    )?;
    validate_community_response_field(
        field_bytes,
        &function.template,
        MAX_SQL_BYTES,
        "Community function template",
    )
}

fn validate_community_function_parameter_list(
    parameters: &wire::CommunityFunctionParameterList,
) -> Result<(), String> {
    if parameters.parameters.len() > MAX_COMMUNITY_ROUTINE_PARAMETERS {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_ROUTINE_PARAMETERS}-routine-parameter limit"
        ));
    }
    let mut field_bytes = 0;
    for parameter in &parameters.parameters {
        for (value, field) in [
            (
                &parameter.function_database,
                "Community function-parameter database",
            ),
            (
                &parameter.function_schema,
                "Community function-parameter schema",
            ),
            (
                &parameter.function_name,
                "Community function-parameter function",
            ),
            (
                &parameter.column_name,
                "Community function-parameter column",
            ),
            (
                &parameter.type_name,
                "Community function-parameter type name",
            ),
            (
                &parameter.is_nullable,
                "Community function-parameter nullable text",
            ),
            (
                &parameter.specific_name,
                "Community function-parameter specific name",
            ),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
        validate_community_response_field(
            &mut field_bytes,
            &parameter.remarks,
            MAX_COMMUNITY_COMMENT_BYTES,
            "Community function-parameter remarks",
        )?;
    }
    validate_community_response_encoded_len(parameters)
}

fn validate_community_procedure_list(
    procedures: &wire::CommunityProcedureList,
) -> Result<(), String> {
    if procedures.procedures.len() > MAX_COMMUNITY_PROCEDURES {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_PROCEDURES}-procedure limit"
        ));
    }
    let mut field_bytes = 0;
    for procedure in &procedures.procedures {
        validate_community_procedure_value(procedure, &mut field_bytes)?;
    }
    validate_community_response_encoded_len(procedures)
}

fn validate_community_procedure(procedure: &wire::CommunityProcedure) -> Result<(), String> {
    let mut field_bytes = 0;
    validate_community_procedure_value(procedure, &mut field_bytes)?;
    validate_community_response_encoded_len(procedure)
}

fn validate_community_procedure_value(
    procedure: &wire::CommunityProcedure,
    field_bytes: &mut usize,
) -> Result<(), String> {
    for (value, field) in [
        (&procedure.database_name, "Community procedure database"),
        (&procedure.schema_name, "Community procedure schema"),
        (&procedure.name, "Community procedure name"),
        (
            &procedure.specific_name,
            "Community procedure specific name",
        ),
    ] {
        validate_community_response_field(field_bytes, value, MAX_SCALAR_BYTES, field)?;
    }
    validate_community_response_field(
        field_bytes,
        &procedure.remarks,
        MAX_COMMUNITY_COMMENT_BYTES,
        "Community procedure remarks",
    )?;
    validate_community_response_field(
        field_bytes,
        &procedure.body,
        MAX_SQL_BYTES,
        "Community procedure body",
    )
}

fn validate_community_procedure_parameter_list(
    parameters: &wire::CommunityProcedureParameterList,
) -> Result<(), String> {
    if parameters.parameters.len() > MAX_COMMUNITY_ROUTINE_PARAMETERS {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_ROUTINE_PARAMETERS}-routine-parameter limit"
        ));
    }
    let mut field_bytes = 0;
    for parameter in &parameters.parameters {
        for (value, field) in [
            (
                &parameter.procedure_database,
                "Community procedure-parameter database",
            ),
            (
                &parameter.procedure_schema,
                "Community procedure-parameter schema",
            ),
            (
                &parameter.procedure_name,
                "Community procedure-parameter procedure",
            ),
            (
                &parameter.column_name,
                "Community procedure-parameter column",
            ),
            (
                &parameter.type_name,
                "Community procedure-parameter type name",
            ),
            (
                &parameter.is_nullable,
                "Community procedure-parameter nullable text",
            ),
            (
                &parameter.specific_name,
                "Community procedure-parameter specific name",
            ),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
        validate_community_response_field(
            &mut field_bytes,
            &parameter.remarks,
            MAX_COMMUNITY_COMMENT_BYTES,
            "Community procedure-parameter remarks",
        )?;
        validate_community_response_field(
            &mut field_bytes,
            &parameter.column_default,
            MAX_SQL_BYTES,
            "Community procedure-parameter default",
        )?;
    }
    validate_community_response_encoded_len(parameters)
}

fn validate_community_trigger_list(triggers: &wire::CommunityTriggerList) -> Result<(), String> {
    if triggers.triggers.len() > MAX_COMMUNITY_TRIGGERS {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_TRIGGERS}-trigger limit"
        ));
    }
    let mut field_bytes = 0;
    for trigger in &triggers.triggers {
        validate_community_trigger_value(trigger, &mut field_bytes)?;
    }
    validate_community_response_encoded_len(triggers)
}

fn validate_community_trigger(trigger: &wire::CommunityTrigger) -> Result<(), String> {
    let mut field_bytes = 0;
    validate_community_trigger_value(trigger, &mut field_bytes)?;
    validate_community_response_encoded_len(trigger)
}

fn validate_community_trigger_value(
    trigger: &wire::CommunityTrigger,
    field_bytes: &mut usize,
) -> Result<(), String> {
    for (value, field) in [
        (&trigger.database_name, "Community trigger database"),
        (&trigger.schema_name, "Community trigger schema"),
        (&trigger.name, "Community trigger name"),
        (
            &trigger.event_manipulation,
            "Community trigger event manipulation",
        ),
    ] {
        validate_community_response_field(field_bytes, value, MAX_SCALAR_BYTES, field)?;
    }
    validate_community_response_field(
        field_bytes,
        &trigger.body,
        MAX_SQL_BYTES,
        "Community trigger body",
    )
}

fn validate_community_table_column_list(
    columns: &wire::CommunityTableColumnList,
) -> Result<(), String> {
    if columns.columns.len() > MAX_COMMUNITY_COLUMNS {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_COLUMNS}-column limit"
        ));
    }
    let mut field_bytes = 0;
    for column in &columns.columns {
        for (value, field) in [
            (&column.database_name, "Community column database"),
            (&column.schema_name, "Community column schema"),
            (&column.table_name, "Community column table"),
            (&column.name, "Community column name"),
            (&column.column_type, "Community column type"),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
        validate_community_response_field(
            &mut field_bytes,
            &column.default_value,
            MAX_SQL_BYTES,
            "Community column default value",
        )?;
        validate_community_response_field(
            &mut field_bytes,
            &column.comment,
            MAX_COMMUNITY_COMMENT_BYTES,
            "Community column comment",
        )?;
        for (value, field) in [
            (
                &column.primary_key_name,
                "Community column primary-key name",
            ),
            (&column.extent, "Community column extent"),
            (&column.charset, "Community column charset"),
            (&column.collation, "Community column collation"),
            (&column.unit, "Community column unit"),
            (
                &column.default_constraint_name,
                "Community column default-constraint name",
            ),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
    }
    validate_community_response_encoded_len(columns)
}

fn validate_community_table_index_list(
    indexes: &wire::CommunityTableIndexList,
) -> Result<(), String> {
    if indexes.indexes.len() > MAX_COMMUNITY_INDEXES {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_INDEXES}-index limit"
        ));
    }
    let mut field_bytes = 0;
    let mut index_column_count = 0_usize;
    for index in &indexes.indexes {
        for (value, field) in [
            (&index.database_name, "Community index database"),
            (&index.schema_name, "Community index schema"),
            (&index.table_name, "Community index table"),
            (&index.name, "Community index name"),
            (&index.r#type, "Community index type"),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
        validate_community_response_field(
            &mut field_bytes,
            &index.comment,
            MAX_COMMUNITY_COMMENT_BYTES,
            "Community index comment",
        )?;
        for (value, field) in [
            (&index.method, "Community index method"),
            (&index.foreign_schema_name, "Community index foreign schema"),
            (&index.foreign_table_name, "Community index foreign table"),
        ] {
            validate_community_response_field(&mut field_bytes, value, MAX_SCALAR_BYTES, field)?;
        }
        add_community_index_columns(&mut index_column_count, index.columns.len())?;
        for column in &index.columns {
            validate_community_table_index_column(column, &mut field_bytes)?;
        }
        add_community_index_columns(&mut index_column_count, index.foreign_column_names.len())?;
        for name in &index.foreign_column_names {
            validate_community_response_field(
                &mut field_bytes,
                name,
                MAX_SCALAR_BYTES,
                "Community index foreign column",
            )?;
        }
    }
    validate_community_response_encoded_len(indexes)
}

fn validate_community_table_index_column(
    column: &wire::CommunityTableIndexColumn,
    field_bytes: &mut usize,
) -> Result<(), String> {
    for (value, field) in [
        (&column.database_name, "Community index-column database"),
        (&column.schema_name, "Community index-column schema"),
        (&column.table_name, "Community index-column table"),
        (&column.index_name, "Community index-column index name"),
        (&column.column_name, "Community index-column name"),
        (&column.r#type, "Community index-column type"),
    ] {
        validate_community_response_field(field_bytes, value, MAX_SCALAR_BYTES, field)?;
    }
    validate_community_response_field(
        field_bytes,
        &column.comment,
        MAX_COMMUNITY_COMMENT_BYTES,
        "Community index-column comment",
    )?;
    for (value, field) in [
        (&column.collation, "Community index-column collation"),
        (&column.index_qualifier, "Community index-column qualifier"),
        (&column.sort_order, "Community index-column sort order"),
    ] {
        validate_community_response_field(field_bytes, value, MAX_SCALAR_BYTES, field)?;
    }
    validate_community_response_field(
        field_bytes,
        &column.filter_condition,
        MAX_SQL_BYTES,
        "Community index-column filter condition",
    )
}

fn add_community_index_columns(total: &mut usize, additional: usize) -> Result<(), String> {
    *total = total
        .checked_add(additional)
        .ok_or_else(|| "Community index-column count overflowed".to_owned())?;
    if *total > MAX_COMMUNITY_INDEX_COLUMNS {
        return Err(format!(
            "Community metadata exceeded the {MAX_COMMUNITY_INDEX_COLUMNS}-index-column limit"
        ));
    }
    Ok(())
}

fn validate_community_response_field(
    total: &mut usize,
    value: &str,
    maximum: usize,
    field: &str,
) -> Result<(), String> {
    validate_bytes_limit(value, maximum, field)?;
    add_community_response_field(total, value)
}

fn validate_community_source_commit(commit: &str) -> Result<(), String> {
    if commit.is_empty() {
        return Ok(());
    }
    if commit.len() != 40
        || commit.len() > MAX_COMMUNITY_SOURCE_COMMIT_BYTES
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Community catalog used an invalid source commit".to_owned());
    }
    Ok(())
}

fn validate_engine_error(error: &wire::EngineError) -> Result<Option<SessionState>, String> {
    validate_scalar(&error.code, "engine error code")?;
    validate_scalar(&error.message, "engine error message")?;
    match wire::ErrorCategory::try_from(error.category) {
        Ok(wire::ErrorCategory::Unspecified) | Err(_) => {
            return Err(format!(
                "engine error used unknown category {}",
                error.category
            ));
        }
        Ok(_) => {}
    }
    match wire::OperationOutcome::try_from(error.outcome) {
        Ok(wire::OperationOutcome::Unspecified) | Err(_) => {
            return Err(format!(
                "engine error used unknown operation outcome {}",
                error.outcome
            ));
        }
        Ok(_) => {}
    }
    for (key, value) in &error.metadata {
        validate_scalar(key, "engine error metadata key")?;
        validate_scalar(value, "engine error metadata value")?;
    }
    if let Some(database) = &error.database_error {
        validate_optional_scalar(database.sql_state.as_deref(), "database SQL state")?;
        validate_optional_scalar(database.constraint_name.as_deref(), "constraint name")?;
        if database.causes.len() > MAX_ERROR_CAUSES {
            return Err(format!(
                "database error exceeded the {MAX_ERROR_CAUSES}-cause limit"
            ));
        }
        for cause in &database.causes {
            validate_scalar(&cause.class_name, "database error class")?;
            validate_scalar(&cause.message, "database error cause message")?;
            validate_optional_scalar(cause.sql_state.as_deref(), "database cause SQL state")?;
        }
    }
    error.session_state.map(SessionState::from_wire).transpose()
}

fn validate_jdbc_value(value: &wire::JdbcValue) -> Result<(), String> {
    use wire::jdbc_value::Value;

    match value.value.as_ref() {
        Some(
            Value::DecimalValue(value)
            | Value::TextValue(value)
            | Value::DateValue(value)
            | Value::TimeValue(value)
            | Value::TimestampValue(value)
            | Value::TimestampWithTimeZoneValue(value)
            | Value::JsonValue(value)
            | Value::UuidValue(value),
        ) => validate_scalar(value, "JDBC row value"),
        Some(Value::BinaryValue(value)) if value.len() > MAX_SCALAR_BYTES => Err(format!(
            "JDBC binary row value exceeded the {MAX_SCALAR_BYTES}-byte scalar limit"
        )),
        Some(Value::OpaqueValue(value)) => {
            validate_scalar(&value.type_name, "opaque JDBC type name")?;
            validate_scalar(&value.display_value, "opaque JDBC display value")
        }
        None => Err("JDBC row value omitted its value".to_owned()),
        Some(_) => Ok(()),
    }
}

fn validate_non_empty_bytes(value: &str, maximum: usize, field: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{field} cannot be empty"));
    }
    if value.len() > maximum {
        return Err(format!("{field} exceeded the {maximum}-byte limit"));
    }
    Ok(())
}

fn validate_scalar(value: &str, field: &str) -> Result<(), String> {
    if value.len() > MAX_SCALAR_BYTES {
        return Err(format!(
            "{field} exceeded the {MAX_SCALAR_BYTES}-byte scalar limit"
        ));
    }
    Ok(())
}

fn validate_bytes_limit(value: &str, maximum: usize, field: &str) -> Result<(), String> {
    if value.len() > maximum {
        return Err(format!("{field} exceeded the {maximum}-byte limit"));
    }
    Ok(())
}

fn add_community_response_field(total: &mut usize, value: &str) -> Result<(), String> {
    *total = total
        .checked_add(value.len())
        .ok_or_else(|| "Community response string byte count overflowed".to_owned())?;
    if *total > MAX_COMMUNITY_RESPONSE_BYTES {
        return Err(format!(
            "Community response string fields exceeded the {MAX_COMMUNITY_RESPONSE_BYTES}-byte limit"
        ));
    }
    Ok(())
}

fn add_optional_community_response_field(
    total: &mut usize,
    value: Option<&str>,
) -> Result<(), String> {
    if let Some(value) = value {
        add_community_response_field(total, value)?;
    }
    Ok(())
}

fn validate_community_response_encoded_len(message: &impl Message) -> Result<(), String> {
    let encoded = message.encoded_len();
    if encoded > MAX_COMMUNITY_RESPONSE_BYTES {
        return Err(format!(
            "Community response encoded length {encoded} exceeded the {MAX_COMMUNITY_RESPONSE_BYTES}-byte limit"
        ));
    }
    Ok(())
}

fn validate_optional_scalar(value: Option<&str>, field: &str) -> Result<(), String> {
    if let Some(value) = value {
        validate_scalar(value, field)?;
    }
    Ok(())
}

#[derive(Default)]
struct RetiredRequests {
    traces: HashMap<String, String>,
    order: VecDeque<String>,
}

impl RetiredRequests {
    fn insert(&mut self, request_id: String, trace_id: String) {
        if self.traces.insert(request_id.clone(), trace_id).is_some() {
            return;
        }
        self.order.push_back(request_id);
        while self.order.len() > MAX_RETIRED_REQUESTS {
            if let Some(expired) = self.order.pop_front() {
                self.traces.remove(&expired);
            }
        }
    }

    fn accept(&mut self, meta: &wire::ResponseMeta, has_payload: bool) -> Result<(), String> {
        let Some(trace_id) = self.traces.remove(&meta.request_id) else {
            return Err(format!(
                "response references unknown request {}",
                meta.request_id
            ));
        };
        self.order.retain(|retired| retired != &meta.request_id);
        if meta.trace_id != trace_id {
            return Err(format!(
                "retired response trace id does not match request {}",
                meta.request_id
            ));
        }
        if meta.sequence != 0 || !meta.terminal || !has_payload {
            return Err(format!(
                "retired request {} received an invalid unary response",
                meta.request_id
            ));
        }
        Ok(())
    }
}

pub(super) fn fail_sink(response: PendingSink, failure: PendingFailure) {
    match response {
        PendingSink::Unary { response, .. } => {
            let _ = response.send(Err(failure));
        }
        PendingSink::Stream { events, .. } => {
            let _ = events.try_send(Err(failure));
        }
    }
}

fn validate_stream_registration(
    response: &PendingSink,
    session_id: Option<&str>,
) -> Result<(), PendingFailure> {
    let PendingSink::Stream {
        event_capacity,
        initial_credits,
        ..
    } = response
    else {
        return Ok(());
    };
    let Some(session_id) = session_id else {
        return Err(PendingFailure::InvalidRequest(
            "query streams require a session id".to_owned(),
        ));
    };
    if session_id.is_empty() {
        return Err(PendingFailure::InvalidRequest(
            "query stream session id cannot be empty".to_owned(),
        ));
    }
    let available_batch_slots = event_capacity.saturating_sub(2);
    if usize::try_from(*initial_credits).unwrap_or(usize::MAX) > available_batch_slots {
        return Err(PendingFailure::InvalidRequest(format!(
            "initial query credits {initial_credits} exceed bounded event capacity {available_batch_slots}"
        )));
    }
    if *initial_credits > MAX_CREDIT_GRANT {
        return Err(PendingFailure::InvalidRequest(format!(
            "initial query credits exceed {MAX_CREDIT_GRANT}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use super::*;

    fn response(
        request_id: &str,
        sequence: u64,
        terminal: bool,
        payload: wire::server_envelope::Payload,
    ) -> wire::ServerEnvelope {
        wire::ServerEnvelope {
            meta: Some(wire::ResponseMeta {
                request_id: request_id.to_owned(),
                trace_id: format!("trace-{request_id}"),
                sequence,
                terminal,
            }),
            payload: Some(payload),
        }
    }

    fn query_started() -> wire::server_envelope::Payload {
        wire::server_envelope::Payload::QueryStarted(wire::QueryStarted {
            columns: vec![column(1)],
        })
    }

    fn column(ordinal: u32) -> wire::JdbcColumn {
        wire::JdbcColumn {
            ordinal,
            label: "value".to_owned(),
            name: "value".to_owned(),
            jdbc_type: 12,
            jdbc_type_name: "VARCHAR".to_owned(),
            value_type: wire::JdbcValueType::Text as i32,
            nullability: wire::ColumnNullability::Nullable as i32,
            ..Default::default()
        }
    }

    fn text_row(values: impl IntoIterator<Item = String>) -> wire::JdbcRow {
        wire::JdbcRow {
            values: values
                .into_iter()
                .map(|value| wire::JdbcValue {
                    value: Some(wire::jdbc_value::Value::TextValue(value)),
                })
                .collect(),
        }
    }

    fn rejected_batch(
        options: wire::QueryOptions,
        columns: Vec<wire::JdbcColumn>,
        batch: wire::RowBatch,
    ) -> String {
        let mut pending = PendingRequests::new();
        let state = Arc::new(SessionStateCell::new(SessionState::AutoCommit));
        let _events = insert_stream(&mut pending, "batch", state, options);
        pending
            .route_response(response(
                "batch",
                0,
                false,
                wire::server_envelope::Payload::QueryStarted(wire::QueryStarted { columns }),
            ))
            .expect("query-started must route");
        pending
            .route_response(response(
                "batch",
                1,
                false,
                wire::server_envelope::Payload::RowBatch(batch),
            ))
            .expect_err("invalid row batch must fail routing")
    }

    fn remote_error(session_state: i32) -> wire::server_envelope::Payload {
        wire::server_envelope::Payload::Error(wire::EngineError {
            code: "fixture.error".to_owned(),
            message: "fixture error".to_owned(),
            category: wire::ErrorCategory::Database as i32,
            outcome: wire::OperationOutcome::KnownFailed as i32,
            session_state: Some(session_state),
            ..Default::default()
        })
    }

    fn insert_stream(
        pending: &mut PendingRequests,
        request_id: &str,
        state: Arc<SessionStateCell>,
        options: wire::QueryOptions,
    ) -> mpsc::Receiver<Result<QueryEvent, PendingFailure>> {
        let (events, receiver) = mpsc::channel(34);
        let result = pending.insert(
            request_id.to_owned(),
            format!("trace-{request_id}"),
            Some("session".to_owned()),
            PendingSink::Stream {
                events,
                event_capacity: 34,
                initial_credits: options.initial_batch_credits,
                session_state: state,
                budgets: QueryBudgets::from_options(&options),
            },
            Instant::now() + Duration::from_secs(1),
            PendingLane::Stream,
        );
        assert!(result.is_ok());
        receiver
    }

    #[test]
    fn stream_error_updates_session_state_before_consumer_observes_it() {
        let mut pending = PendingRequests::new();
        let state = Arc::new(SessionStateCell::new(SessionState::TransactionActive));
        let mut events = insert_stream(
            &mut pending,
            "query",
            state.clone(),
            wire::QueryOptions::default(),
        );
        pending
            .route_response(response("query", 0, false, query_started()))
            .expect("query-started must route");
        pending
            .route_response(response(
                "query",
                1,
                true,
                remote_error(wire::SessionState::RollbackRequired as i32),
            ))
            .expect("terminal error must route");

        assert_eq!(state.get(), SessionState::RollbackRequired);
        state.set(SessionState::AutoCommit);
        assert!(matches!(events.try_recv(), Ok(Ok(QueryEvent::Started(_)))));
        assert!(matches!(
            events.try_recv(),
            Ok(Err(PendingFailure::Remote(_)))
        ));
        assert_eq!(state.get(), SessionState::AutoCommit);
    }

    #[test]
    fn abandoned_stream_error_still_updates_session_state() {
        let mut pending = PendingRequests::new();
        let state = Arc::new(SessionStateCell::new(SessionState::TransactionActive));
        let _events = insert_stream(
            &mut pending,
            "query",
            state.clone(),
            wire::QueryOptions::default(),
        );
        pending.abandon_stream("query", "session");
        pending
            .route_response(response(
                "query",
                0,
                true,
                remote_error(wire::SessionState::RollbackRequired as i32),
            ))
            .expect("abandoned terminal error must still route");
        assert_eq!(state.get(), SessionState::RollbackRequired);
    }

    #[test]
    fn stream_budgets_and_unknown_error_enums_are_protocol_failures() {
        let mut pending = PendingRequests::new();
        let state = Arc::new(SessionStateCell::new(SessionState::AutoCommit));
        let _events = insert_stream(
            &mut pending,
            "rows",
            state.clone(),
            wire::QueryOptions {
                max_rows: 1,
                initial_batch_credits: 1,
                ..Default::default()
            },
        );
        pending
            .route_response(response("rows", 0, false, query_started()))
            .expect("query-started must route");
        let row = wire::JdbcRow {
            values: vec![wire::JdbcValue {
                value: Some(wire::jdbc_value::Value::TextValue("value".to_owned())),
            }],
        };
        let error = pending
            .route_response(response(
                "rows",
                1,
                false,
                wire::server_envelope::Payload::RowBatch(wire::RowBatch {
                    start_row_offset: 0,
                    rows: vec![row.clone(), row],
                }),
            ))
            .expect_err("max_rows overflow must fail routing");
        assert!(error.contains("max_rows"));

        let mut pending = PendingRequests::new();
        let _events = insert_stream(&mut pending, "error", state, wire::QueryOptions::default());
        let error = pending
            .route_response(response(
                "error",
                0,
                true,
                wire::server_envelope::Payload::Error(wire::EngineError {
                    code: "bad.enum".to_owned(),
                    message: "bad enum".to_owned(),
                    category: 999,
                    outcome: wire::OperationOutcome::KnownFailed as i32,
                    ..Default::default()
                }),
            ))
            .expect_err("unknown error category must fail routing");
        assert!(error.contains("unknown category"));
    }

    #[test]
    fn stream_enforces_column_scalar_and_batch_hard_limits() {
        let mut pending = PendingRequests::new();
        let state = Arc::new(SessionStateCell::new(SessionState::AutoCommit));
        let _events = insert_stream(
            &mut pending,
            "columns",
            state.clone(),
            wire::QueryOptions::default(),
        );
        let columns = (1..=MAX_COLUMNS + 1)
            .map(|ordinal| column(u32::try_from(ordinal).unwrap_or(u32::MAX)))
            .collect();
        let error = pending
            .route_response(response(
                "columns",
                0,
                false,
                wire::server_envelope::Payload::QueryStarted(wire::QueryStarted { columns }),
            ))
            .expect_err("column hard limit must fail routing");
        assert!(error.contains("column limit"));

        let mut pending = PendingRequests::new();
        let _events = insert_stream(&mut pending, "scalar", state, wire::QueryOptions::default());
        let mut oversized = column(1);
        oversized.label = "x".repeat(MAX_SCALAR_BYTES + 1);
        let error = pending
            .route_response(response(
                "scalar",
                0,
                false,
                wire::server_envelope::Payload::QueryStarted(wire::QueryStarted {
                    columns: vec![oversized],
                }),
            ))
            .expect_err("scalar hard limit must fail routing");
        assert!(error.contains("scalar limit"));

        let row = text_row(["x".to_owned()]);
        let error = rejected_batch(
            wire::QueryOptions {
                initial_batch_credits: 1,
                ..Default::default()
            },
            vec![column(1)],
            wire::RowBatch {
                start_row_offset: 0,
                rows: vec![row; MAX_BATCH_ROWS + 1],
            },
        );
        assert!(error.contains("row batch limit"));

        let large = "x".repeat(MAX_SCALAR_BYTES);
        let error = rejected_batch(
            wire::QueryOptions {
                initial_batch_credits: 1,
                ..Default::default()
            },
            vec![column(1), column(2), column(3)],
            wire::RowBatch {
                start_row_offset: 0,
                rows: vec![text_row([large.clone(), large.clone(), large])],
            },
        );
        assert!(error.contains("byte batch limit"));
    }

    #[test]
    fn stream_enforces_requested_batch_and_result_budgets() {
        let row = text_row(["value".to_owned()]);
        let error = rejected_batch(
            wire::QueryOptions {
                target_batch_rows: 1,
                initial_batch_credits: 1,
                ..Default::default()
            },
            vec![column(1)],
            wire::RowBatch {
                start_row_offset: 0,
                rows: vec![row.clone(), row.clone()],
            },
        );
        assert!(error.contains("target_batch_rows"));

        let error = rejected_batch(
            wire::QueryOptions {
                target_batch_bytes: 1_024,
                initial_batch_credits: 1,
                ..Default::default()
            },
            vec![column(1)],
            wire::RowBatch {
                start_row_offset: 0,
                rows: vec![text_row(["x".repeat(2_048)])],
            },
        );
        assert!(error.contains("target_batch_bytes"));

        let error = rejected_batch(
            wire::QueryOptions {
                max_result_bytes: 1,
                initial_batch_credits: 1,
                ..Default::default()
            },
            vec![column(1)],
            wire::RowBatch {
                start_row_offset: 0,
                rows: vec![row],
            },
        );
        assert!(error.contains("max_result_bytes"));
    }

    #[test]
    fn stream_rejects_unspecified_error_outcome_and_session_state() {
        for (outcome, session_state, expected) in [
            (
                wire::OperationOutcome::Unspecified as i32,
                wire::SessionState::Broken as i32,
                "unknown operation outcome",
            ),
            (
                wire::OperationOutcome::KnownFailed as i32,
                wire::SessionState::Unspecified as i32,
                "unknown JDBC session state",
            ),
        ] {
            let mut pending = PendingRequests::new();
            let state = Arc::new(SessionStateCell::new(SessionState::AutoCommit));
            let _events =
                insert_stream(&mut pending, "error", state, wire::QueryOptions::default());
            let error = pending
                .route_response(response(
                    "error",
                    0,
                    true,
                    wire::server_envelope::Payload::Error(wire::EngineError {
                        code: "bad.enum".to_owned(),
                        message: "bad enum".to_owned(),
                        category: wire::ErrorCategory::Database as i32,
                        outcome,
                        session_state: Some(session_state),
                        ..Default::default()
                    }),
                ))
                .expect_err("unspecified wire enums must fail routing");
            assert!(error.contains(expected), "unexpected error: {error}");
        }
    }

    #[test]
    fn community_responses_enforce_generated_bounds() {
        let invalid_commit =
            wire::server_envelope::Payload::CommunityPluginCatalog(wire::CommunityPluginCatalog {
                source_commit: "ABC".to_owned(),
                ..Default::default()
            });
        assert!(
            validate_response_payload(&invalid_commit)
                .expect_err("invalid Community source commit must fail")
                .contains("invalid source commit")
        );

        let oversized_catalog =
            wire::server_envelope::Payload::CommunityPluginCatalog(wire::CommunityPluginCatalog {
                plugins: vec![
                    wire::CommunityPluginDescriptor::default();
                    MAX_COMMUNITY_PLUGINS + 1
                ],
                ..Default::default()
            });
        assert!(
            validate_response_payload(&oversized_catalog)
                .expect_err("oversized Community plugin catalog must fail")
                .contains("plugin limit")
        );

        let empty_sql =
            wire::server_envelope::Payload::CommunityBuiltSql(wire::CommunityBuiltSql::default());
        assert!(
            validate_response_payload(&empty_sql)
                .expect_err("empty Community SQL must fail")
                .contains("cannot be empty")
        );

        let oversized_comment =
            wire::server_envelope::Payload::CommunitySchemaList(wire::CommunitySchemaList {
                schemas: vec![wire::CommunitySchema {
                    comment: "x".repeat(MAX_COMMUNITY_COMMENT_BYTES + 1),
                    ..Default::default()
                }],
            });
        assert!(
            validate_response_payload(&oversized_comment)
                .expect_err("oversized Community schema comment must fail")
                .contains("comment")
        );
    }

    #[test]
    fn community_object_metadata_responses_enforce_collection_bounds() {
        let oversized_databases =
            wire::server_envelope::Payload::CommunityDatabaseList(wire::CommunityDatabaseList {
                databases: vec![wire::CommunityDatabase::default(); MAX_COMMUNITY_DATABASES + 1],
            });
        assert!(
            validate_response_payload(&oversized_databases)
                .expect_err("oversized Community database list must fail")
                .contains("database limit")
        );
        drop(oversized_databases);

        let oversized_tables =
            wire::server_envelope::Payload::CommunityTableList(wire::CommunityTableList {
                tables: vec![wire::CommunityTable::default(); MAX_COMMUNITY_TABLES + 1],
            });
        assert!(
            validate_response_payload(&oversized_tables)
                .expect_err("oversized Community table list must fail")
                .contains("table limit")
        );
        drop(oversized_tables);

        let oversized_columns = wire::server_envelope::Payload::CommunityTableColumnList(
            wire::CommunityTableColumnList {
                columns: vec![wire::CommunityTableColumn::default(); MAX_COMMUNITY_COLUMNS + 1],
            },
        );
        assert!(
            validate_response_payload(&oversized_columns)
                .expect_err("oversized Community column list must fail")
                .contains("column limit")
        );
        drop(oversized_columns);

        let oversized_indexes = wire::server_envelope::Payload::CommunityTableIndexList(
            wire::CommunityTableIndexList {
                indexes: vec![wire::CommunityTableIndex::default(); MAX_COMMUNITY_INDEXES + 1],
            },
        );
        assert!(
            validate_response_payload(&oversized_indexes)
                .expect_err("oversized Community index list must fail")
                .contains("index limit")
        );
    }

    #[test]
    fn community_relation_metadata_responses_enforce_collection_bounds() {
        let oversized_views =
            wire::server_envelope::Payload::CommunityViewList(wire::CommunityViewList {
                views: vec![wire::CommunityTable::default(); MAX_COMMUNITY_VIEWS + 1],
            });
        assert!(
            validate_response_payload(&oversized_views)
                .expect_err("oversized Community view list must fail")
                .contains("view limit")
        );
        drop(oversized_views);

        let oversized_imported = wire::server_envelope::Payload::CommunityImportedKeyList(
            wire::CommunityForeignKeyList {
                keys: vec![wire::CommunityForeignKey::default(); MAX_COMMUNITY_KEYS + 1],
            },
        );
        assert!(
            validate_response_payload(&oversized_imported)
                .expect_err("oversized Community foreign-key list must fail")
                .contains("foreign-key limit")
        );
        drop(oversized_imported);

        let oversized_primary = wire::server_envelope::Payload::CommunityPrimaryKeyList(
            wire::CommunityPrimaryKeyList {
                keys: vec![wire::CommunityPrimaryKey::default(); MAX_COMMUNITY_KEYS + 1],
            },
        );
        assert!(
            validate_response_payload(&oversized_primary)
                .expect_err("oversized Community primary-key list must fail")
                .contains("primary-key limit")
        );
    }

    #[test]
    fn community_programmability_responses_enforce_collection_bounds() {
        let oversized_functions =
            wire::server_envelope::Payload::CommunityFunctionList(wire::CommunityFunctionList {
                functions: vec![wire::CommunityFunction::default(); MAX_COMMUNITY_FUNCTIONS + 1],
            });
        assert!(
            validate_response_payload(&oversized_functions)
                .expect_err("oversized Community function list must fail")
                .contains("function limit")
        );
        drop(oversized_functions);

        let oversized_function_parameters =
            wire::server_envelope::Payload::CommunityFunctionParameterList(
                wire::CommunityFunctionParameterList {
                    parameters: vec![
                        wire::CommunityFunctionParameter::default();
                        MAX_COMMUNITY_ROUTINE_PARAMETERS + 1
                    ],
                },
            );
        assert!(
            validate_response_payload(&oversized_function_parameters)
                .expect_err("oversized Community function-parameter list must fail")
                .contains("routine-parameter limit")
        );
        drop(oversized_function_parameters);

        let oversized_procedures =
            wire::server_envelope::Payload::CommunityProcedureList(wire::CommunityProcedureList {
                procedures: vec![wire::CommunityProcedure::default(); MAX_COMMUNITY_PROCEDURES + 1],
            });
        assert!(
            validate_response_payload(&oversized_procedures)
                .expect_err("oversized Community procedure list must fail")
                .contains("procedure limit")
        );
        drop(oversized_procedures);

        let oversized_procedure_parameters =
            wire::server_envelope::Payload::CommunityProcedureParameterList(
                wire::CommunityProcedureParameterList {
                    parameters: vec![
                        wire::CommunityProcedureParameter::default();
                        MAX_COMMUNITY_ROUTINE_PARAMETERS + 1
                    ],
                },
            );
        assert!(
            validate_response_payload(&oversized_procedure_parameters)
                .expect_err("oversized Community procedure-parameter list must fail")
                .contains("routine-parameter limit")
        );
        drop(oversized_procedure_parameters);

        let oversized_triggers =
            wire::server_envelope::Payload::CommunityTriggerList(wire::CommunityTriggerList {
                triggers: vec![wire::CommunityTrigger::default(); MAX_COMMUNITY_TRIGGERS + 1],
            });
        assert!(
            validate_response_payload(&oversized_triggers)
                .expect_err("oversized Community trigger list must fail")
                .contains("trigger limit")
        );
    }

    #[test]
    fn community_programmability_responses_enforce_field_bounds() {
        let oversized_function =
            wire::server_envelope::Payload::CommunityFunction(wire::CommunityFunction {
                template: "x".repeat(MAX_SQL_BYTES + 1),
                ..Default::default()
            });
        assert!(
            validate_response_payload(&oversized_function)
                .expect_err("oversized Community function template must fail")
                .contains("function template")
        );

        let oversized_function_parameter =
            wire::server_envelope::Payload::CommunityFunctionParameterList(
                wire::CommunityFunctionParameterList {
                    parameters: vec![wire::CommunityFunctionParameter {
                        type_name: "x".repeat(MAX_SCALAR_BYTES + 1),
                        ..Default::default()
                    }],
                },
            );
        assert!(
            validate_response_payload(&oversized_function_parameter)
                .expect_err("oversized Community function-parameter type must fail")
                .contains("function-parameter type name")
        );

        let oversized_procedure =
            wire::server_envelope::Payload::CommunityProcedure(wire::CommunityProcedure {
                remarks: "x".repeat(MAX_COMMUNITY_COMMENT_BYTES + 1),
                ..Default::default()
            });
        assert!(
            validate_response_payload(&oversized_procedure)
                .expect_err("oversized Community procedure remarks must fail")
                .contains("procedure remarks")
        );

        let oversized_procedure_parameter =
            wire::server_envelope::Payload::CommunityProcedureParameterList(
                wire::CommunityProcedureParameterList {
                    parameters: vec![wire::CommunityProcedureParameter {
                        column_default: "x".repeat(MAX_SQL_BYTES + 1),
                        ..Default::default()
                    }],
                },
            );
        assert!(
            validate_response_payload(&oversized_procedure_parameter)
                .expect_err("oversized Community procedure-parameter default must fail")
                .contains("procedure-parameter default")
        );

        let oversized_trigger =
            wire::server_envelope::Payload::CommunityTrigger(wire::CommunityTrigger {
                body: "x".repeat(MAX_SQL_BYTES + 1),
                ..Default::default()
            });
        assert!(
            validate_response_payload(&oversized_trigger)
                .expect_err("oversized Community trigger body must fail")
                .contains("trigger body")
        );

        let oversized_trigger_list =
            wire::server_envelope::Payload::CommunityTriggerList(wire::CommunityTriggerList {
                triggers: vec![wire::CommunityTrigger {
                    name: "x".repeat(MAX_SCALAR_BYTES + 1),
                    ..Default::default()
                }],
            });
        assert!(
            validate_response_payload(&oversized_trigger_list)
                .expect_err("oversized Community trigger name in a list must fail")
                .contains("trigger name")
        );
    }

    #[test]
    fn community_relation_metadata_responses_enforce_field_bounds() {
        let oversized_view_ddl =
            wire::server_envelope::Payload::CommunityViewList(wire::CommunityViewList {
                views: vec![wire::CommunityTable {
                    ddl: "x".repeat(MAX_SQL_BYTES + 1),
                    ..Default::default()
                }],
            });
        assert!(
            validate_response_payload(&oversized_view_ddl)
                .expect_err("oversized Community view DDL must fail")
                .contains("table DDL")
        );

        let oversized_foreign_column = wire::server_envelope::Payload::CommunityExportedKeyList(
            wire::CommunityForeignKeyList {
                keys: vec![wire::CommunityForeignKey {
                    foreign_column_name: "x".repeat(MAX_SCALAR_BYTES + 1),
                    ..Default::default()
                }],
            },
        );
        assert!(
            validate_response_payload(&oversized_foreign_column)
                .expect_err("oversized Community foreign-key field must fail")
                .contains("foreign column")
        );

        let oversized_primary_name = wire::server_envelope::Payload::CommunityPrimaryKeyList(
            wire::CommunityPrimaryKeyList {
                keys: vec![wire::CommunityPrimaryKey {
                    name: "x".repeat(MAX_SCALAR_BYTES + 1),
                    ..Default::default()
                }],
            },
        );
        assert!(
            validate_response_payload(&oversized_primary_name)
                .expect_err("oversized Community primary-key field must fail")
                .contains("primary-key name")
        );
    }

    #[test]
    fn community_object_metadata_responses_enforce_field_and_nested_bounds() {
        let oversized_database_comment =
            wire::server_envelope::Payload::CommunityDatabaseList(wire::CommunityDatabaseList {
                databases: vec![wire::CommunityDatabase {
                    comment: "x".repeat(MAX_COMMUNITY_COMMENT_BYTES + 1),
                    ..Default::default()
                }],
            });
        assert!(
            validate_response_payload(&oversized_database_comment)
                .expect_err("oversized Community database comment must fail")
                .contains("database comment")
        );

        let oversized_table_ddl =
            wire::server_envelope::Payload::CommunityTableList(wire::CommunityTableList {
                tables: vec![wire::CommunityTable {
                    ddl: "x".repeat(MAX_SQL_BYTES + 1),
                    ..Default::default()
                }],
            });
        assert!(
            validate_response_payload(&oversized_table_ddl)
                .expect_err("oversized Community table DDL must fail")
                .contains("table DDL")
        );

        let oversized_column_name = wire::server_envelope::Payload::CommunityTableColumnList(
            wire::CommunityTableColumnList {
                columns: vec![wire::CommunityTableColumn {
                    name: "x".repeat(MAX_SCALAR_BYTES + 1),
                    ..Default::default()
                }],
            },
        );
        assert!(
            validate_response_payload(&oversized_column_name)
                .expect_err("oversized Community column name must fail")
                .contains("column name")
        );

        let oversized_index_filter = wire::server_envelope::Payload::CommunityTableIndexList(
            wire::CommunityTableIndexList {
                indexes: vec![wire::CommunityTableIndex {
                    columns: vec![wire::CommunityTableIndexColumn {
                        filter_condition: "x".repeat(MAX_SQL_BYTES + 1),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            },
        );
        assert!(
            validate_response_payload(&oversized_index_filter)
                .expect_err("oversized Community index filter must fail")
                .contains("filter condition")
        );

        let excessive_nested_columns = wire::server_envelope::Payload::CommunityTableIndexList(
            wire::CommunityTableIndexList {
                indexes: vec![wire::CommunityTableIndex {
                    columns: vec![
                        wire::CommunityTableIndexColumn::default();
                        MAX_COMMUNITY_INDEX_COLUMNS
                    ],
                    foreign_column_names: vec![String::new()],
                    ..Default::default()
                }],
            },
        );
        assert!(
            validate_response_payload(&excessive_nested_columns)
                .expect_err("combined Community index-column limit must fail")
                .contains("index-column limit")
        );
    }

    #[test]
    fn community_responses_enforce_aggregate_and_encoded_byte_budgets() {
        let aggregate_overflow =
            wire::server_envelope::Payload::CommunityPluginCatalog(wire::CommunityPluginCatalog {
                source_commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                plugins: vec![wire::CommunityPluginDescriptor {
                    database_type: "H2".to_owned(),
                    name: "H2".to_owned(),
                    drivers: vec![wire::CommunityDriverConfig {
                        url: "u".repeat(MAX_SCALAR_BYTES),
                        jdbc_driver: "d".repeat(MAX_SCALAR_BYTES),
                        jdbc_driver_class: "c".to_owned(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            });
        assert!(
            validate_response_payload(&aggregate_overflow)
                .expect_err("aggregate Community strings above the budget must fail")
                .contains("string fields")
        );

        let mut boundary = wire::CommunitySqlAnalysis {
            statements: vec![wire::CommunityParsedStatement {
                sql: "SELECT 1".to_owned(),
                r#type: "t".repeat(MAX_SCALAR_BYTES),
                ..Default::default()
            }],
            ..Default::default()
        };
        let remaining = MAX_COMMUNITY_RESPONSE_BYTES
            .checked_sub(boundary.encoded_len())
            .expect("boundary fixture must leave encoded space");
        boundary.statements[0].statement_type = "s".repeat(remaining);
        while boundary.encoded_len() > MAX_COMMUNITY_RESPONSE_BYTES {
            boundary.statements[0].statement_type.pop();
        }
        while boundary.encoded_len() < MAX_COMMUNITY_RESPONSE_BYTES {
            boundary.statements[0].statement_type.push('s');
        }
        assert_eq!(boundary.encoded_len(), MAX_COMMUNITY_RESPONSE_BYTES);
        validate_response_payload(&wire::server_envelope::Payload::CommunitySqlAnalysis(
            boundary.clone(),
        ))
        .expect("a Community response exactly at the encoded budget must pass");

        boundary.statements[0].statement_type.push('s');
        assert_eq!(boundary.encoded_len(), MAX_COMMUNITY_RESPONSE_BYTES + 1);
        assert!(
            validate_response_payload(&wire::server_envelope::Payload::CommunitySqlAnalysis(
                boundary,
            ))
            .expect_err("a Community response one encoded byte over budget must fail")
            .contains("encoded length")
        );
    }

    #[test]
    fn community_sql_validation_responses_enforce_counts_and_string_bounds() {
        let too_many_statements =
            wire::server_envelope::Payload::CommunitySqlValidation(wire::CommunitySqlValidation {
                statements: vec![
                    wire::CommunityParsedStatement::default();
                    MAX_COMMUNITY_STATEMENTS + 1
                ],
                ..Default::default()
            });
        assert!(
            validate_response_payload(&too_many_statements)
                .expect_err("validation statement count above the limit must fail")
                .contains("statement limit")
        );

        let too_many_diagnostics =
            wire::server_envelope::Payload::CommunitySqlValidation(wire::CommunitySqlValidation {
                diagnostics: vec![
                    wire::CommunitySqlDiagnostic::default();
                    MAX_COMMUNITY_SQL_DIAGNOSTICS + 1
                ],
                ..Default::default()
            });
        assert!(
            validate_response_payload(&too_many_diagnostics)
                .expect_err("diagnostic count above the limit must fail")
                .contains("diagnostic limit")
        );

        for (diagnostic, expected) in [
            (
                wire::CommunitySqlDiagnostic {
                    token_text: "x".repeat(MAX_SQL_BYTES + 1),
                    ..Default::default()
                },
                "diagnostic token text",
            ),
            (
                wire::CommunitySqlDiagnostic {
                    message: "x".repeat(MAX_COMMUNITY_COMMENT_BYTES + 1),
                    ..Default::default()
                },
                "diagnostic message",
            ),
        ] {
            let payload = wire::server_envelope::Payload::CommunitySqlValidation(
                wire::CommunitySqlValidation {
                    diagnostics: vec![diagnostic],
                    ..Default::default()
                },
            );
            assert!(
                validate_response_payload(&payload)
                    .expect_err("oversized diagnostic string must fail")
                    .contains(expected)
            );
        }
    }

    #[test]
    fn community_sql_validation_requires_validity_to_match_diagnostics() {
        for validation in [
            wire::CommunitySqlValidation {
                valid: true,
                diagnostics: vec![wire::CommunitySqlDiagnostic::default()],
                ..Default::default()
            },
            wire::CommunitySqlValidation {
                valid: false,
                diagnostics: Vec::new(),
                ..Default::default()
            },
        ] {
            assert!(
                validate_response_payload(&wire::server_envelope::Payload::CommunitySqlValidation(
                    validation
                ))
                .expect_err("inconsistent validation validity must fail")
                .contains("validity disagreed")
            );
        }
    }

    #[test]
    fn community_sql_validation_enforces_cumulative_and_encoded_byte_budgets() {
        let aggregate_overflow =
            wire::server_envelope::Payload::CommunitySqlValidation(wire::CommunitySqlValidation {
                statements: vec![
                    wire::CommunityParsedStatement {
                        sql: "x".repeat(MAX_SQL_BYTES),
                        ..Default::default()
                    };
                    (MAX_COMMUNITY_RESPONSE_BYTES / MAX_SQL_BYTES) + 1
                ],
                ..Default::default()
            });
        assert!(
            validate_response_payload(&aggregate_overflow)
                .expect_err("aggregate validation strings above the budget must fail")
                .contains("string fields")
        );

        let bytes_per_diagnostic = MAX_COMMUNITY_RESPONSE_BYTES / MAX_COMMUNITY_SQL_DIAGNOSTICS;
        let encoded_overflow =
            wire::server_envelope::Payload::CommunitySqlValidation(wire::CommunitySqlValidation {
                diagnostics: vec![
                    wire::CommunitySqlDiagnostic {
                        message: "x".repeat(bytes_per_diagnostic),
                        ..Default::default()
                    };
                    MAX_COMMUNITY_SQL_DIAGNOSTICS
                ],
                ..Default::default()
            });
        assert!(
            validate_response_payload(&encoded_overflow)
                .expect_err("validation framing above the encoded budget must fail")
                .contains("encoded length")
        );
    }

    #[test]
    fn community_formatted_sql_enforces_nonempty_one_megabyte_output() {
        let empty = wire::server_envelope::Payload::CommunityFormattedSql(
            wire::CommunityFormattedSql::default(),
        );
        assert!(
            validate_response_payload(&empty)
                .expect_err("empty formatted SQL must fail")
                .contains("cannot be empty")
        );

        let exact =
            wire::server_envelope::Payload::CommunityFormattedSql(wire::CommunityFormattedSql {
                sql: "x".repeat(MAX_SQL_BYTES),
            });
        validate_response_payload(&exact).expect("exact formatted-SQL byte limit must pass");

        let oversized =
            wire::server_envelope::Payload::CommunityFormattedSql(wire::CommunityFormattedSql {
                sql: "x".repeat(MAX_SQL_BYTES + 1),
            });
        assert!(
            validate_response_payload(&oversized)
                .expect_err("formatted SQL above one MiB must fail")
                .contains(&format!("{MAX_SQL_BYTES}-byte limit"))
        );
    }

    #[test]
    fn community_built_dml_enforces_nonempty_one_megabyte_output() {
        let empty =
            wire::server_envelope::Payload::CommunityBuiltDml(wire::CommunityBuiltDml::default());
        assert!(
            validate_response_payload(&empty)
                .expect_err("empty built DML must fail")
                .contains("cannot be empty")
        );

        let exact = wire::server_envelope::Payload::CommunityBuiltDml(wire::CommunityBuiltDml {
            sql: "x".repeat(MAX_SQL_BYTES),
        });
        validate_response_payload(&exact).expect("exact built-DML byte limit must pass");

        let oversized =
            wire::server_envelope::Payload::CommunityBuiltDml(wire::CommunityBuiltDml {
                sql: "x".repeat(MAX_SQL_BYTES + 1),
            });
        assert!(
            validate_response_payload(&oversized)
                .expect_err("built DML above one MiB must fail")
                .contains(&format!("{MAX_SQL_BYTES}-byte limit"))
        );
    }

    #[test]
    fn community_built_namespace_sql_enforces_nonempty_one_megabyte_output() {
        let empty = wire::server_envelope::Payload::CommunityBuiltNamespaceSql(
            wire::CommunityBuiltNamespaceSql::default(),
        );
        assert!(
            validate_response_payload(&empty)
                .expect_err("empty built namespace SQL must fail")
                .contains("cannot be empty")
        );

        let exact = wire::server_envelope::Payload::CommunityBuiltNamespaceSql(
            wire::CommunityBuiltNamespaceSql {
                sql: "x".repeat(MAX_SQL_BYTES),
            },
        );
        validate_response_payload(&exact).expect("exact built-namespace-SQL byte limit must pass");

        let oversized = wire::server_envelope::Payload::CommunityBuiltNamespaceSql(
            wire::CommunityBuiltNamespaceSql {
                sql: "x".repeat(MAX_SQL_BYTES + 1),
            },
        );
        assert!(
            validate_response_payload(&oversized)
                .expect_err("built namespace SQL above one MiB must fail")
                .contains(&format!("{MAX_SQL_BYTES}-byte limit"))
        );
    }

    fn valid_completion_candidate() -> wire::CommunitySqlCompletionCandidate {
        wire::CommunitySqlCompletionCandidate {
            id: Some("table:customer".to_owned()),
            label: "CUSTOMER".to_owned(),
            r#type: "TABLE".to_owned(),
            insert_text: Some("CUSTOMER".to_owned()),
            insert_type: "PLAIN_TEXT".to_owned(),
            detail: Some("APP.CUSTOMER".to_owned()),
            snippet_slots: vec!["INSERT_COLUMN_LIST".to_owned()],
            ..Default::default()
        }
    }

    fn valid_completion() -> wire::CommunitySqlCompletion {
        wire::CommunitySqlCompletion {
            status: "SUCCESS".to_owned(),
            replace_start_utf16: 14,
            replace_end_utf16: 14,
            candidates: vec![valid_completion_candidate()],
            editor_hints: vec![wire::CommunitySqlCompletionEditorHint {
                r#type: "INSERT_VALUE".to_owned(),
                statement_range: Some(wire::CommunitySqlCompletionRange {
                    start_line_number: 1,
                    start_column: 1,
                    end_line_number: 1,
                    end_column: 15,
                }),
                items: vec![wire::CommunitySqlCompletionEditorHintItem {
                    row_index: 0,
                    column_index: 0,
                    label: Some("id".to_owned()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn community_sql_completion_accepts_the_bounded_projection() {
        validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
            valid_completion(),
        ))
        .expect("valid completion projection must pass");
    }

    #[test]
    fn community_sql_completion_rejects_unknown_enum_strings_and_invalid_ranges() {
        for mutate in [
            |completion: &mut wire::CommunitySqlCompletion| completion.status = "MAYBE".to_owned(),
            |completion: &mut wire::CommunitySqlCompletion| {
                completion.candidates[0].r#type = "UNKNOWN_TYPE".to_owned();
            },
            |completion: &mut wire::CommunitySqlCompletion| {
                completion.candidates[0].insert_type = "TEMPLATE".to_owned();
            },
            |completion: &mut wire::CommunitySqlCompletion| {
                completion.candidates[0].snippet_slots = vec!["OTHER_SLOT".to_owned()];
            },
            |completion: &mut wire::CommunitySqlCompletion| {
                completion.editor_hints[0].r#type = "OTHER_HINT".to_owned();
            },
        ] {
            let mut completion = valid_completion();
            mutate(&mut completion);
            assert!(
                validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
                    completion
                ))
                .expect_err("unknown completion enum string must fail")
                .contains("unknown value")
            );
        }

        let mut reversed = valid_completion();
        reversed.replace_start_utf16 = 15;
        reversed.replace_end_utf16 = 14;
        assert!(
            validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
                reversed
            ))
            .expect_err("reversed completion replacement must fail")
            .contains("start exceeds")
        );

        let mut zero_based = valid_completion();
        zero_based.editor_hints[0]
            .statement_range
            .as_mut()
            .expect("test range must exist")
            .start_column = 0;
        assert!(
            validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
                zero_based
            ))
            .expect_err("editor ranges must be one-based")
            .contains("one-based")
        );
    }

    #[test]
    fn community_sql_completion_enforces_all_collection_limits_after_decode() {
        let too_many_candidates = wire::CommunitySqlCompletion {
            status: "EMPTY".to_owned(),
            candidates: vec![
                wire::CommunitySqlCompletionCandidate::default();
                MAX_COMMUNITY_SQL_COMPLETION_CANDIDATES + 1
            ],
            ..Default::default()
        };
        assert!(
            validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
                too_many_candidates
            ))
            .expect_err("candidate limit plus one must fail")
            .contains("candidate limit")
        );

        let too_many_hints = wire::CommunitySqlCompletion {
            status: "EMPTY".to_owned(),
            editor_hints: vec![
                wire::CommunitySqlCompletionEditorHint::default();
                MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINTS + 1
            ],
            ..Default::default()
        };
        assert!(
            validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
                too_many_hints
            ))
            .expect_err("editor-hint limit plus one must fail")
            .contains("editor-hint limit")
        );

        let mut too_many_slots = valid_completion();
        too_many_slots.candidates[0].snippet_slots =
            vec!["SELECT_FUNCTION".to_owned(); MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS + 1];
        assert!(
            validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
                too_many_slots
            ))
            .expect_err("snippet-slot limit plus one must fail")
            .contains("snippet-slot limit")
        );

        let mut too_many_items = valid_completion();
        too_many_items.editor_hints[0].items = vec![
            wire::CommunitySqlCompletionEditorHintItem::default();
            MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS + 1
        ];
        assert!(
            validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
                too_many_items
            ))
            .expect_err("editor-hint-item limit plus one must fail")
            .contains("editor-hint-item limit")
        );
    }

    #[test]
    fn community_sql_completion_enforces_the_cumulative_string_budget() {
        let mut completion = valid_completion();
        completion.candidates[0].id = Some("i".repeat(MAX_SCALAR_BYTES));
        completion.candidates[0].detail = Some("d".repeat(MAX_SCALAR_BYTES));
        assert!(
            validate_response_payload(&wire::server_envelope::Payload::CommunitySqlCompletion(
                completion
            ))
            .expect_err("aggregate completion strings above eight MiB must fail")
            .contains("string fields")
        );
    }

    #[test]
    fn duplicate_cancel_is_rejected_until_the_first_cancel_completes() {
        let mut pending = PendingRequests::new();
        let state = Arc::new(SessionStateCell::new(SessionState::AutoCommit));
        let _events = insert_stream(
            &mut pending,
            "query",
            state.clone(),
            wire::QueryOptions::default(),
        );
        let insert_cancel = |pending: &mut PendingRequests, request_id: &str| {
            let (response, _receiver) = oneshot::channel();
            pending.insert(
                request_id.to_owned(),
                format!("trace-{request_id}"),
                Some("session".to_owned()),
                PendingSink::Unary {
                    response,
                    session_state: Some(state.clone()),
                },
                Instant::now() + Duration::from_secs(1),
                PendingLane::Control(ControlEffect::Cancel {
                    target_request_id: "query".to_owned(),
                }),
            )
        };

        assert!(insert_cancel(&mut pending, "cancel-1").is_ok());
        let Err((_sink, failure)) = insert_cancel(&mut pending, "cancel-2") else {
            panic!("duplicate cancel must be rejected");
        };
        assert!(matches!(failure, PendingFailure::InvalidRequest(_)));
        pending
            .route_response(response(
                "cancel-1",
                0,
                true,
                wire::server_envelope::Payload::OperationCancelled(wire::OperationCancelled {
                    disposition: wire::CancelDisposition::Accepted as i32,
                }),
            ))
            .expect("first cancel must complete");
        assert!(insert_cancel(&mut pending, "cancel-3").is_ok());
    }
}
