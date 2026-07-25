import type { components } from '../generated/contract';

type Schema<Name extends keyof components['schemas']> = components['schemas'][Name];

export type AgentMessageList = Schema<'AgentMessageList'>;
export type AgentSession = Schema<'AgentSession'>;
export type AgentSessionList = Schema<'AgentSessionList'>;
export type ApiError = Schema<'ApiError'>;
export type CancelOperationResponse = Schema<'CancelOperationResponse'>;
export type CreateAgentSessionRequest = Schema<'CreateAgentSessionRequest'>;
export type CreateDatasourceRequest = Schema<'CreateDatasourceRequest'>;
export type CreateProviderProfileRequest = Schema<'CreateProviderProfileRequest'>;
export type Datasource = Schema<'Datasource'>;
export type DatasourceConnection = Schema<'DatasourceConnection'>;
export type DatasourceConnectionProperty = Schema<'DatasourceConnectionProperty'>;
export type DatasourceList = Schema<'DatasourceList'>;
export type DatasourceSecretChange = Schema<'DatasourceSecretChange'>;
export type HealthResponse = Schema<'HealthResponse'>;
export type JdbcDriver = Schema<'JdbcDriver'>;
export type JdbcDriverList = Schema<'JdbcDriverList'>;
export type JdbcValue = Schema<'JdbcValue'>;
export type OperationEventEnvelope = Schema<'OperationEventEnvelope'>;
export type OperationSnapshot = Schema<'OperationSnapshot'>;
export type OperationStreamMessage = Schema<'OperationStreamMessage'>;
export type OperationSubscriptionAccepted = Schema<'OperationSubscriptionAccepted'>;
export type ProviderProfile = Schema<'ProviderProfile'>;
export type ProviderProfileList = Schema<'ProviderProfileList'>;
export type QueryAccepted = Schema<'QueryAccepted'>;
export type ResultPage = Schema<'ResultPage'>;
export type ResultPageRequest = Schema<'ResultPageRequest'>;
export type StartQueryRequest = Schema<'StartQueryRequest'>;
export type UpdateAgentSessionRequest = Schema<'UpdateAgentSessionRequest'>;
export type UpdateDatasourceRequest = Schema<'UpdateDatasourceRequest'>;
export type UpdateProviderProfileRequest = Schema<'UpdateProviderProfileRequest'>;

export type AgentEvent = Schema<'AgentEvent'>;
export type AgentEventEnvelope = Schema<'AgentEventEnvelope'>;
export type AgentPermissionDecision = Schema<'AgentPermissionDecision'>;
export type AgentPermissionRequest = Schema<'AgentPermissionRequest'>;
export type AgentPermissionResponse = Schema<'AgentPermissionResponse'>;
export type AgentPermissionStatus = Schema<'AgentPermissionStatus'>;
export type AgentResultHandle = Schema<'AgentResultHandle'>;
export type AgentRunAccepted = Schema<'AgentRunAccepted'>;
export type AgentRunSnapshot = Schema<'AgentRunSnapshot'>;
export type AgentRunStatus = Schema<'AgentRunStatus'>;
export type AgentStreamMessage = Schema<'AgentStreamMessage'>;
export type AgentSubscriptionAccepted = Schema<'AgentSubscriptionAccepted'>;
export type AgentToolOutput = Schema<'AgentToolOutput'>;
export type AgentUsage = Schema<'AgentUsage'>;
export type CancelAgentRunResponse = Schema<'CancelAgentRunResponse'>;
export type ContextCompactionStrategy = Schema<'ContextCompactionStrategy'>;
export type DecideAgentPermissionRequest = Schema<'DecideAgentPermissionRequest'>;
export type SqlPermissionMode = Schema<'SqlPermissionMode'>;
export type StartAgentRunRequest = Schema<'StartAgentRunRequest'>;

export interface OperationSubscriptionOptions {
  afterSequence?: string;
  signal?: AbortSignal;
  onEvent: (event: OperationEventEnvelope) => void;
  onError?: (error: Error) => void;
  onClose?: () => void;
}

export interface OperationSubscription {
  close(): void;
}

export interface AgentSubscriptionOptions {
  afterSequence?: string;
  signal?: AbortSignal;
  onEvent: (event: AgentEventEnvelope) => void;
  onError?: (error: Error) => void;
  onClose?: () => void;
}

export interface AgentSubscription {
  close(): void;
}

export interface BackendClient {
  readonly transport: 'http' | 'tauri';
  health(signal?: AbortSignal): Promise<HealthResponse>;
  listDrivers(signal?: AbortSignal): Promise<JdbcDriverList>;
  listDatasources(signal?: AbortSignal): Promise<DatasourceList>;
  createDatasource(
    request: CreateDatasourceRequest,
    signal?: AbortSignal,
  ): Promise<Datasource>;
  getDatasource(datasourceId: string, signal?: AbortSignal): Promise<Datasource>;
  updateDatasource(
    datasourceId: string,
    request: UpdateDatasourceRequest,
    signal?: AbortSignal,
  ): Promise<Datasource>;
  deleteDatasource(
    datasourceId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void>;
  listProviderProfiles(signal?: AbortSignal): Promise<ProviderProfileList>;
  createProviderProfile(
    request: CreateProviderProfileRequest,
    signal?: AbortSignal,
  ): Promise<ProviderProfile>;
  getProviderProfile(providerId: string, signal?: AbortSignal): Promise<ProviderProfile>;
  updateProviderProfile(
    providerId: string,
    request: UpdateProviderProfileRequest,
    signal?: AbortSignal,
  ): Promise<ProviderProfile>;
  deleteProviderProfile(
    providerId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void>;
  listAgentSessions(signal?: AbortSignal): Promise<AgentSessionList>;
  createAgentSession(
    request: CreateAgentSessionRequest,
    signal?: AbortSignal,
  ): Promise<AgentSession>;
  getAgentSession(sessionId: string, signal?: AbortSignal): Promise<AgentSession>;
  updateAgentSession(
    sessionId: string,
    request: UpdateAgentSessionRequest,
    signal?: AbortSignal,
  ): Promise<AgentSession>;
  deleteAgentSession(
    sessionId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void>;
  listAgentMessages(
    sessionId: string,
    startOrdinal: string,
    limit: string,
    signal?: AbortSignal,
  ): Promise<AgentMessageList>;
  startAgentRun(
    request: StartAgentRunRequest,
    signal?: AbortSignal,
  ): Promise<AgentRunAccepted>;
  agentRunSnapshot(runId: string, signal?: AbortSignal): Promise<AgentRunSnapshot>;
  cancelAgentRun(runId: string, signal?: AbortSignal): Promise<CancelAgentRunResponse>;
  decideAgentPermission(
    permissionId: string,
    request: DecideAgentPermissionRequest,
    signal?: AbortSignal,
  ): Promise<AgentPermissionResponse>;
  subscribeAgentRun(
    runId: string,
    options: AgentSubscriptionOptions,
  ): Promise<AgentSubscription>;
  startQuery(request: StartQueryRequest, signal?: AbortSignal): Promise<QueryAccepted>;
  operationSnapshot(operationId: string, signal?: AbortSignal): Promise<OperationSnapshot>;
  cancelOperation(operationId: string, signal?: AbortSignal): Promise<CancelOperationResponse>;
  subscribeOperation(
    operationId: string,
    options: OperationSubscriptionOptions,
  ): Promise<OperationSubscription>;
  resultPage(
    resultId: string,
    request: ResultPageRequest,
    signal?: AbortSignal,
  ): Promise<ResultPage>;
}

export class ApiRequestError extends Error {
  readonly apiError: ApiError;
  readonly status?: number;

  constructor(apiError: ApiError, status?: number) {
    super(apiError.message);
    this.name = 'ApiRequestError';
    this.apiError = apiError;
    this.status = status;
  }
}

const MAX_U64 = 18_446_744_073_709_551_615n;

export function parseOperationCursor(value: string): bigint {
  if (!/^\d+$/.test(value)) {
    throw new ApiRequestError({
      code: 'invalid_last_event_id',
      message: 'Last-Event-ID must be an unsigned decimal integer',
      retryable: false,
    });
  }
  const sequence = BigInt(value);
  if (sequence > MAX_U64) {
    throw new ApiRequestError({
      code: 'invalid_last_event_id',
      message: 'Last-Event-ID must be an unsigned decimal integer',
      retryable: false,
    });
  }
  return sequence;
}

export function parseAgentCursor(value: string): bigint {
  return parseOperationCursor(value);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isNullableString(value: unknown): boolean {
  return value === undefined || value === null || typeof value === 'string';
}

function isNullableNumber(value: unknown): boolean {
  return value === undefined || value === null || typeof value === 'number';
}

function isApiErrorDetails(value: unknown): boolean {
  if (!isRecord(value) || typeof value.type !== 'string') return false;

  switch (value.type) {
    case 'revision_conflict':
      return typeof value.expectedRevision === 'string'
        && isNullableString(value.actualRevision);
    case 'database':
      return isNullableString(value.sqlState)
        && isNullableNumber(value.vendorCode)
        && isNullableNumber(value.statementPosition)
        && isNullableString(value.constraintName);
    case 'replay_window':
      return typeof value.requestedSequence === 'string'
        && typeof value.oldestAvailableSequence === 'string'
        && typeof value.latestSequence === 'string';
    default:
      return false;
  }
}

export function isApiError(value: unknown): value is ApiError {
  if (!isRecord(value)
    || typeof value.code !== 'string'
    || typeof value.message !== 'string'
    || (value.retryable !== undefined && typeof value.retryable !== 'boolean')) {
    return false;
  }
  return value.details === undefined
    || value.details === null
    || isApiErrorDetails(value.details);
}

export function protocolError(message: string, status?: number): ApiRequestError {
  return new ApiRequestError({ code: 'invalid_transport_response', message, retryable: false }, status);
}

export function normalizeApiError(value: unknown): ApiRequestError {
  if (value instanceof ApiRequestError) return value;
  if (isApiError(value)) return new ApiRequestError(value);
  return protocolError('The runtime returned an invalid error response');
}

function isDecimalInteger(value: unknown): value is string {
  return typeof value === 'string' && /^(0|[1-9]\d*)$/.test(value);
}

function isOptionalNullableString(value: unknown): boolean {
  return value === undefined || value === null || typeof value === 'string';
}

function isI32(value: unknown): value is number {
  return typeof value === 'number'
    && Number.isInteger(value)
    && value >= -2_147_483_648
    && value <= 2_147_483_647;
}

function isU32(value: unknown): value is number {
  return typeof value === 'number'
    && Number.isInteger(value)
    && value >= 0
    && value <= 4_294_967_295;
}

function isOptionalNullableI32(value: unknown): boolean {
  return value === undefined || value === null || isI32(value);
}

function isOptionalNullableU32(value: unknown): boolean {
  return value === undefined || value === null || isU32(value);
}

function isOptionalNullableBoolean(value: unknown): boolean {
  return value === undefined || value === null || typeof value === 'boolean';
}

function isJdbcValue(value: unknown): value is JdbcValue {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  if (value.type === 'null') return true;
  if (value.type === 'boolean') return typeof value.value === 'boolean';
  if (value.type === 'opaque') {
    return typeof value.typeName === 'string' && typeof value.displayValue === 'string';
  }
  return [
    'signed_integer',
    'unsigned_integer',
    'float32',
    'float64',
    'decimal',
    'text',
    'binary',
    'date',
    'time',
    'timestamp',
    'timestamp_with_time_zone',
    'json',
    'uuid',
  ].includes(value.type) && typeof value.value === 'string';
}

function isResultColumn(value: unknown): value is Schema<'ResultColumn'> {
  return isRecord(value)
    && isOptionalNullableString(value.catalogName)
    && isOptionalNullableU32(value.displaySize)
    && isI32(value.jdbcType)
    && typeof value.jdbcTypeName === 'string'
    && typeof value.label === 'string'
    && typeof value.name === 'string'
    && typeof value.nullability === 'string'
    && ['unknown', 'no_nulls', 'nullable'].includes(value.nullability)
    && isU32(value.ordinal)
    && isOptionalNullableU32(value.precision)
    && isOptionalNullableI32(value.scale)
    && isOptionalNullableString(value.schemaName)
    && isOptionalNullableBoolean(value.signed)
    && isOptionalNullableString(value.tableName)
    && typeof value.valueType === 'string'
    && [
      'boolean',
      'signed_integer',
      'unsigned_integer',
      'float32',
      'float64',
      'decimal',
      'text',
      'binary',
      'date',
      'time',
      'timestamp',
      'timestamp_with_time_zone',
      'json',
      'uuid',
      'opaque',
    ].includes(value.valueType);
}

function isResultRow(value: unknown): value is Schema<'ResultRow'> {
  return isRecord(value)
    && Array.isArray(value.values)
    && value.values.every(isJdbcValue);
}

function isAgentRunStatus(value: unknown): value is AgentRunStatus {
  return typeof value === 'string'
    && ['running', 'waiting_for_permission', 'completed', 'failed', 'cancelled'].includes(value);
}

function isAgentPermissionStatus(value: unknown): value is AgentPermissionStatus {
  return typeof value === 'string'
    && ['pending', 'approved', 'denied', 'consumed', 'expired', 'revoked'].includes(value);
}

export function isAgentUsage(value: unknown): value is AgentUsage {
  return isRecord(value)
    && isDecimalInteger(value.inputTokens)
    && isDecimalInteger(value.outputTokens)
    && isDecimalInteger(value.totalTokens);
}

export function isAgentPermissionRequest(value: unknown): value is AgentPermissionRequest {
  return isRecord(value)
    && typeof value.permissionId === 'string'
    && typeof value.runId === 'string'
    && typeof value.toolCallId === 'string'
    && typeof value.toolName === 'string'
    && typeof value.argumentsSha256 === 'string'
    && typeof value.summary === 'string'
    && isDecimalInteger(value.requestedAtMs)
    && isDecimalInteger(value.expiresAtMs);
}

export function isAgentPermissionResponse(value: unknown): value is AgentPermissionResponse {
  return isRecord(value)
    && typeof value.permissionId === 'string'
    && isAgentPermissionStatus(value.status);
}

export function isAgentToolOutput(value: unknown): value is AgentToolOutput {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  if (value.type === 'text') {
    return typeof value.content === 'string' && typeof value.truncated === 'boolean';
  }
  if (value.type !== 'result' || !isRecord(value.handle)) return false;
  const handle = value.handle;
  return typeof handle.handleId === 'string'
    && isDecimalInteger(handle.rowCount)
    && isDecimalInteger(handle.byteCount)
    && typeof handle.truncatedByMaxRows === 'boolean'
    && typeof handle.truncatedByMaxResultBytes === 'boolean'
    && isDecimalInteger(handle.createdAtMs)
    && isDecimalInteger(handle.expiresAtMs)
    && Array.isArray(handle.columns)
    && handle.columns.every(isResultColumn)
    && typeof handle.columnsTruncated === 'boolean'
    && Array.isArray(handle.sampleRows)
    && handle.sampleRows.every(isResultRow)
    && typeof handle.sampleTruncated === 'boolean';
}

function isAgentEvent(value: unknown): value is AgentEvent {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  switch (value.type) {
    case 'started':
      return true;
    case 'text_delta':
      return typeof value.delta === 'string';
    case 'tool_started':
      return typeof value.toolCallId === 'string'
        && typeof value.name === 'string'
        && typeof value.argumentsSha256 === 'string';
    case 'tool_completed':
      return typeof value.toolCallId === 'string'
        && typeof value.name === 'string'
        && isAgentToolOutput(value.output);
    case 'tool_failed':
      return typeof value.toolCallId === 'string'
        && typeof value.name === 'string'
        && isApiError(value.error);
    case 'permission_requested':
      return isAgentPermissionRequest(value.permission);
    case 'permission_resolved':
      return typeof value.permissionId === 'string'
        && isAgentPermissionStatus(value.status);
    case 'context_compacted':
      return (value.strategy === 'summary' || value.strategy === 'deterministic_trim')
        && isDecimalInteger(value.droppedTurns);
    case 'usage':
      return isAgentUsage(value.usage);
    case 'completed':
      return typeof value.messageId === 'string';
    case 'failed':
      return isApiError(value.error);
    case 'cancelled':
      return isOptionalNullableString(value.reason);
    default:
      return false;
  }
}

export function isAgentEventEnvelope(value: unknown): value is AgentEventEnvelope {
  return isRecord(value)
    && typeof value.runId === 'string'
    && isDecimalInteger(value.sequence)
    && isDecimalInteger(value.occurredAtMs)
    && isAgentEvent(value.event);
}

export function isAgentStreamMessage(value: unknown): value is AgentStreamMessage {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  switch (value.type) {
    case 'event':
      return isAgentEventEnvelope(value.event);
    case 'error':
      return isApiError(value.error);
    case 'end':
      return true;
    default:
      return false;
  }
}

export function isAgentSubscriptionAccepted(
  value: unknown,
): value is AgentSubscriptionAccepted {
  return isRecord(value)
    && typeof value.subscriptionId === 'string'
    && value.subscriptionId.length > 0;
}

export function isAgentRunAccepted(value: unknown): value is AgentRunAccepted {
  return isRecord(value)
    && typeof value.runId === 'string'
    && typeof value.sessionId === 'string';
}

export function isAgentRunSnapshot(value: unknown): value is AgentRunSnapshot {
  return isRecord(value)
    && typeof value.runId === 'string'
    && typeof value.sessionId === 'string'
    && isAgentRunStatus(value.status)
    && isDecimalInteger(value.lastSequence)
    && isDecimalInteger(value.startedAtMs)
    && isDecimalInteger(value.updatedAtMs)
    && isDecimalInteger(value.modelRounds)
    && isDecimalInteger(value.toolCalls)
    && isAgentUsage(value.usage)
    && (value.pendingPermission === undefined
      || value.pendingPermission === null
      || isAgentPermissionRequest(value.pendingPermission))
    && isOptionalNullableString(value.messageId)
    && (value.error === undefined || value.error === null || isApiError(value.error));
}

export function isCancelAgentRunResponse(value: unknown): value is CancelAgentRunResponse {
  return isRecord(value)
    && typeof value.runId === 'string'
    && (value.disposition === 'accepted'
      || value.disposition === 'already_terminal'
      || value.disposition === 'unknown_operation');
}

function isOperationEvent(value: unknown): boolean {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  switch (value.type) {
    case 'started':
      return true;
    case 'progress':
      return isDecimalInteger(value.rowCount) && isDecimalInteger(value.byteCount);
    case 'completed':
      return isRecord(value.result)
        && typeof value.result.id === 'string'
        && isDecimalInteger(value.result.rowCount)
        && isDecimalInteger(value.result.byteCount);
    case 'failed':
      return isApiError(value.error);
    case 'cancelled':
      return value.reason === undefined || value.reason === null || typeof value.reason === 'string';
    default:
      return false;
  }
}

export function isOperationEventEnvelope(value: unknown): value is OperationEventEnvelope {
  return isRecord(value)
    && typeof value.operationId === 'string'
    && isDecimalInteger(value.sequence)
    && isDecimalInteger(value.occurredAtMs)
    && isOperationEvent(value.event);
}

export function isOperationStreamMessage(value: unknown): value is OperationStreamMessage {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  switch (value.type) {
    case 'event':
      return isOperationEventEnvelope(value.event);
    case 'error':
      return isApiError(value.error);
    case 'end':
      return true;
    default:
      return false;
  }
}

export function isOperationSubscriptionAccepted(
  value: unknown,
): value is OperationSubscriptionAccepted {
  return isRecord(value)
    && typeof value.subscriptionId === 'string'
    && value.subscriptionId.length > 0;
}

export function abortError(signal: AbortSignal): DOMException {
  return signal.reason instanceof DOMException
    ? signal.reason
    : new DOMException('The operation was aborted', 'AbortError');
}

export function withAbort<T>(promise: Promise<T>, signal?: AbortSignal): Promise<T> {
  if (!signal) return promise;
  if (signal.aborted) return Promise.reject(abortError(signal));

  return new Promise<T>((resolve, reject) => {
    const abort = () => reject(abortError(signal));
    signal.addEventListener('abort', abort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener('abort', abort);
        resolve(value);
      },
      (error: unknown) => {
        signal.removeEventListener('abort', abort);
        reject(error);
      },
    );
  });
}
