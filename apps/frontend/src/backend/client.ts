import type { components } from '../generated/contract';

type Schema<Name extends keyof components['schemas']> = components['schemas'][Name];

export type ApiError = Schema<'ApiError'>;
export type CancelOperationResponse = Schema<'CancelOperationResponse'>;
export type CreateDatasourceRequest = Schema<'CreateDatasourceRequest'>;
export type Datasource = Schema<'Datasource'>;
export type DatasourceConnection = Schema<'DatasourceConnection'>;
export type DatasourceConnectionProperty = Schema<'DatasourceConnectionProperty'>;
export type DatasourceList = Schema<'DatasourceList'>;
export type DatasourceSecretChange = Schema<'DatasourceSecretChange'>;
export type HealthResponse = Schema<'HealthResponse'>;
export type JdbcValue = Schema<'JdbcValue'>;
export type OperationEventEnvelope = Schema<'OperationEventEnvelope'>;
export type OperationSnapshot = Schema<'OperationSnapshot'>;
export type OperationStreamMessage = Schema<'OperationStreamMessage'>;
export type OperationSubscriptionAccepted = Schema<'OperationSubscriptionAccepted'>;
export type QueryAccepted = Schema<'QueryAccepted'>;
export type ResultPage = Schema<'ResultPage'>;
export type ResultPageRequest = Schema<'ResultPageRequest'>;
export type StartQueryRequest = Schema<'StartQueryRequest'>;
export type UpdateDatasourceRequest = Schema<'UpdateDatasourceRequest'>;

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

export interface BackendClient {
  readonly transport: 'http' | 'tauri';
  health(signal?: AbortSignal): Promise<HealthResponse>;
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
