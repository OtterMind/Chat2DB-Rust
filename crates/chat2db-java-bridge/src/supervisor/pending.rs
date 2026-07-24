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
        _ => Ok(None),
    }
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
