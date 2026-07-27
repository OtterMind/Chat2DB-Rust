import { createParser } from 'eventsource-parser';

import type {
  AgentEventEnvelope,
  AgentMessageList,
  AgentPermissionResponse,
  AgentRunAccepted,
  AgentRunSnapshot,
  AgentSession,
  AgentSessionList,
  AgentSubscription,
  AgentSubscriptionOptions,
  BackendClient,
  BuildCommunityCreateSchemaRequest,
  BuildCommunityDmlRequest,
  BuildCommunityNamespaceSqlRequest,
  CancelAgentRunResponse,
  CancelOperationResponse,
  CommunityBuiltSql,
  CommunityDatabaseList,
  CommunityFormattedSql,
  CommunityForeignKeyList,
  CommunityFunction,
  CommunityFunctionList,
  CommunityFunctionParameterList,
  CommunityPluginCatalog,
  CommunityPrimaryKeyList,
  CommunityProcedure,
  CommunityProcedureList,
  CommunityProcedureParameterList,
  CommunitySchemaList,
  CommunitySqlAnalysis,
  CommunitySqlCompletion,
  CommunitySqlValidation,
  CommunityTableColumnList,
  CommunityTableIndexList,
  CommunityTableList,
  CommunityTrigger,
  CommunityTriggerList,
  CommunityViewList,
  CreateAgentSessionRequest,
  CreateDatasourceRequest,
  CreateProviderProfileRequest,
  CompleteCommunitySqlRequest,
  Datasource,
  DatasourceList,
  DecideAgentPermissionRequest,
  GetCommunityFunctionRequest,
  GetCommunityProcedureRequest,
  GetCommunityTriggerRequest,
  FormatCommunitySqlRequest,
  HealthResponse,
  JdbcDriverList,
  ListCommunityColumnsRequest,
  ListCommunityDatabasesRequest,
  ListCommunityFunctionsRequest,
  ListCommunityIndexesRequest,
  ListCommunityProceduresRequest,
  ListCommunitySchemasRequest,
  ListCommunityTableKeysRequest,
  ListCommunityTablesRequest,
  ListCommunityTriggersRequest,
  ListCommunityViewsRequest,
  OperationSnapshot,
  OperationSubscription,
  OperationSubscriptionOptions,
  ParseCommunitySqlRequest,
  ProviderProfile,
  ProviderProfileList,
  QueryAccepted,
  ResultPage,
  ResultPageRequest,
  StartAgentRunRequest,
  StartQueryRequest,
  UpdateAgentSessionRequest,
  UpdateDatasourceRequest,
  UpdateProviderProfileRequest,
  ValidateCommunitySqlRequest,
} from './client';
import {
  ApiRequestError,
  isAgentEventEnvelope,
  isAgentPermissionResponse,
  isAgentRunAccepted,
  isAgentRunSnapshot,
  isApiError,
  isCancelAgentRunResponse,
  isOperationEventEnvelope,
  parseAgentCursor,
  parseOperationCursor,
  protocolError,
} from './client';

type Fetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

interface HttpBackendOptions {
  baseUrl?: string;
  fetch?: Fetch;
}

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.endsWith('/') ? baseUrl.slice(0, -1) : baseUrl;
}

async function parseJson(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    throw protocolError('The runtime returned malformed JSON', response.status);
  }
}

async function apiFailure(response: Response): Promise<ApiRequestError> {
  const payload = await parseJson(response);
  if (!isApiError(payload)) {
    return protocolError('The runtime returned an invalid error response', response.status);
  }
  return new ApiRequestError(payload, response.status);
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}

export class HttpBackendClient implements BackendClient {
  readonly transport = 'http' as const;
  readonly #baseUrl: string;
  readonly #fetch: Fetch;

  constructor(options: HttpBackendOptions = {}) {
    this.#baseUrl = normalizeBaseUrl(options.baseUrl ?? '');
    this.#fetch = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  async #json<T>(
    path: string,
    init: RequestInit,
    acceptedStatuses: readonly number[],
  ): Promise<T> {
    const headers = new Headers(init.headers);
    headers.set('Accept', 'application/json');
    if (init.body !== undefined) headers.set('Content-Type', 'application/json');
    const response = await this.#fetch(`${this.#baseUrl}${path}`, { ...init, headers });
    if (!acceptedStatuses.includes(response.status)) throw await apiFailure(response);
    if (response.status === 204) return undefined as T;
    return await parseJson(response) as T;
  }

  health(signal?: AbortSignal): Promise<HealthResponse> {
    return this.#json('/api/v1/system/health', { method: 'GET', signal }, [200, 503]);
  }

  listDrivers(signal?: AbortSignal): Promise<JdbcDriverList> {
    return this.#json('/api/v1/drivers', { method: 'GET', signal }, [200]);
  }

  listCommunityPlugins(signal?: AbortSignal): Promise<CommunityPluginCatalog> {
    return this.#json('/api/v1/community/plugins', { method: 'GET', signal }, [200]);
  }

  listCommunitySchemas(
    request: ListCommunitySchemasRequest,
    signal?: AbortSignal,
  ): Promise<CommunitySchemaList> {
    return this.#json(
      '/api/v1/community/metadata/schemas',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityDatabases(
    request: ListCommunityDatabasesRequest,
    signal?: AbortSignal,
  ): Promise<CommunityDatabaseList> {
    return this.#json(
      '/api/v1/community/metadata/databases',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityTables(
    request: ListCommunityTablesRequest,
    signal?: AbortSignal,
  ): Promise<CommunityTableList> {
    return this.#json(
      '/api/v1/community/metadata/tables',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityColumns(
    request: ListCommunityColumnsRequest,
    signal?: AbortSignal,
  ): Promise<CommunityTableColumnList> {
    return this.#json(
      '/api/v1/community/metadata/columns',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityIndexes(
    request: ListCommunityIndexesRequest,
    signal?: AbortSignal,
  ): Promise<CommunityTableIndexList> {
    return this.#json(
      '/api/v1/community/metadata/indexes',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityViews(
    request: ListCommunityViewsRequest,
    signal?: AbortSignal,
  ): Promise<CommunityViewList> {
    return this.#json(
      '/api/v1/community/metadata/views',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityImportedKeys(
    request: ListCommunityTableKeysRequest,
    signal?: AbortSignal,
  ): Promise<CommunityForeignKeyList> {
    return this.#json(
      '/api/v1/community/metadata/imported-keys',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityExportedKeys(
    request: ListCommunityTableKeysRequest,
    signal?: AbortSignal,
  ): Promise<CommunityForeignKeyList> {
    return this.#json(
      '/api/v1/community/metadata/exported-keys',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityPrimaryKeys(
    request: ListCommunityTableKeysRequest,
    signal?: AbortSignal,
  ): Promise<CommunityPrimaryKeyList> {
    return this.#json(
      '/api/v1/community/metadata/primary-keys',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityFunctions(
    request: ListCommunityFunctionsRequest,
    signal?: AbortSignal,
  ): Promise<CommunityFunctionList> {
    return this.#json(
      '/api/v1/community/metadata/functions',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  getCommunityFunction(
    request: GetCommunityFunctionRequest,
    signal?: AbortSignal,
  ): Promise<CommunityFunction> {
    return this.#json(
      '/api/v1/community/metadata/function',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityFunctionParameters(
    request: GetCommunityFunctionRequest,
    signal?: AbortSignal,
  ): Promise<CommunityFunctionParameterList> {
    return this.#json(
      '/api/v1/community/metadata/function-parameters',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityProcedures(
    request: ListCommunityProceduresRequest,
    signal?: AbortSignal,
  ): Promise<CommunityProcedureList> {
    return this.#json(
      '/api/v1/community/metadata/procedures',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  getCommunityProcedure(
    request: GetCommunityProcedureRequest,
    signal?: AbortSignal,
  ): Promise<CommunityProcedure> {
    return this.#json(
      '/api/v1/community/metadata/procedure',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityProcedureParameters(
    request: GetCommunityProcedureRequest,
    signal?: AbortSignal,
  ): Promise<CommunityProcedureParameterList> {
    return this.#json(
      '/api/v1/community/metadata/procedure-parameters',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listCommunityTriggers(
    request: ListCommunityTriggersRequest,
    signal?: AbortSignal,
  ): Promise<CommunityTriggerList> {
    return this.#json(
      '/api/v1/community/metadata/triggers',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  getCommunityTrigger(
    request: GetCommunityTriggerRequest,
    signal?: AbortSignal,
  ): Promise<CommunityTrigger> {
    return this.#json(
      '/api/v1/community/metadata/trigger',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  buildCommunityCreateSchema(
    request: BuildCommunityCreateSchemaRequest,
    signal?: AbortSignal,
  ): Promise<CommunityBuiltSql> {
    return this.#json(
      '/api/v1/community/sql/build-create-schema',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  buildCommunityDml(
    request: BuildCommunityDmlRequest,
    signal?: AbortSignal,
  ): Promise<CommunityBuiltSql> {
    return this.#json(
      '/api/v1/community/sql/build-dml',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  buildCommunityNamespaceSql(
    request: BuildCommunityNamespaceSqlRequest,
    signal?: AbortSignal,
  ): Promise<CommunityBuiltSql> {
    return this.#json(
      '/api/v1/community/sql/build-namespace',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  parseCommunitySql(
    request: ParseCommunitySqlRequest,
    signal?: AbortSignal,
  ): Promise<CommunitySqlAnalysis> {
    return this.#json(
      '/api/v1/community/sql/parse',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  validateCommunitySql(
    request: ValidateCommunitySqlRequest,
    signal?: AbortSignal,
  ): Promise<CommunitySqlValidation> {
    return this.#json(
      '/api/v1/community/sql/validate',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  formatCommunitySql(
    request: FormatCommunitySqlRequest,
    signal?: AbortSignal,
  ): Promise<CommunityFormattedSql> {
    return this.#json(
      '/api/v1/community/sql/format',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  completeCommunitySql(
    request: CompleteCommunitySqlRequest,
    signal?: AbortSignal,
  ): Promise<CommunitySqlCompletion> {
    return this.#json(
      '/api/v1/community/sql/complete',
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
  }

  listDatasources(signal?: AbortSignal): Promise<DatasourceList> {
    return this.#json('/api/v1/datasources', { method: 'GET', signal }, [200]);
  }

  createDatasource(request: CreateDatasourceRequest, signal?: AbortSignal): Promise<Datasource> {
    return this.#json(
      '/api/v1/datasources',
      { method: 'POST', body: JSON.stringify(request), signal },
      [201],
    );
  }

  getDatasource(datasourceId: string, signal?: AbortSignal): Promise<Datasource> {
    return this.#json(
      `/api/v1/datasources/${encodeURIComponent(datasourceId)}`,
      { method: 'GET', signal },
      [200],
    );
  }

  updateDatasource(
    datasourceId: string,
    request: UpdateDatasourceRequest,
    signal?: AbortSignal,
  ): Promise<Datasource> {
    return this.#json(
      `/api/v1/datasources/${encodeURIComponent(datasourceId)}`,
      { method: 'PUT', body: JSON.stringify(request), signal },
      [200],
    );
  }

  deleteDatasource(
    datasourceId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void> {
    const query = new URLSearchParams({ expectedRevision });
    return this.#json(
      `/api/v1/datasources/${encodeURIComponent(datasourceId)}?${query}`,
      { method: 'DELETE', signal },
      [204],
    );
  }

  listProviderProfiles(signal?: AbortSignal): Promise<ProviderProfileList> {
    return this.#json('/api/v1/agent/providers', { method: 'GET', signal }, [200]);
  }

  createProviderProfile(
    request: CreateProviderProfileRequest,
    signal?: AbortSignal,
  ): Promise<ProviderProfile> {
    return this.#json(
      '/api/v1/agent/providers',
      { method: 'POST', body: JSON.stringify(request), signal },
      [201],
    );
  }

  getProviderProfile(providerId: string, signal?: AbortSignal): Promise<ProviderProfile> {
    return this.#json(
      `/api/v1/agent/providers/${encodeURIComponent(providerId)}`,
      { method: 'GET', signal },
      [200],
    );
  }

  updateProviderProfile(
    providerId: string,
    request: UpdateProviderProfileRequest,
    signal?: AbortSignal,
  ): Promise<ProviderProfile> {
    return this.#json(
      `/api/v1/agent/providers/${encodeURIComponent(providerId)}`,
      { method: 'PUT', body: JSON.stringify(request), signal },
      [200],
    );
  }

  deleteProviderProfile(
    providerId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void> {
    const query = new URLSearchParams({ expectedRevision });
    return this.#json(
      `/api/v1/agent/providers/${encodeURIComponent(providerId)}?${query}`,
      { method: 'DELETE', signal },
      [204],
    );
  }

  listAgentSessions(signal?: AbortSignal): Promise<AgentSessionList> {
    return this.#json('/api/v1/agent/sessions', { method: 'GET', signal }, [200]);
  }

  createAgentSession(
    request: CreateAgentSessionRequest,
    signal?: AbortSignal,
  ): Promise<AgentSession> {
    return this.#json(
      '/api/v1/agent/sessions',
      { method: 'POST', body: JSON.stringify(request), signal },
      [201],
    );
  }

  getAgentSession(sessionId: string, signal?: AbortSignal): Promise<AgentSession> {
    return this.#json(
      `/api/v1/agent/sessions/${encodeURIComponent(sessionId)}`,
      { method: 'GET', signal },
      [200],
    );
  }

  updateAgentSession(
    sessionId: string,
    request: UpdateAgentSessionRequest,
    signal?: AbortSignal,
  ): Promise<AgentSession> {
    return this.#json(
      `/api/v1/agent/sessions/${encodeURIComponent(sessionId)}`,
      { method: 'PUT', body: JSON.stringify(request), signal },
      [200],
    );
  }

  deleteAgentSession(
    sessionId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void> {
    const query = new URLSearchParams({ expectedRevision });
    return this.#json(
      `/api/v1/agent/sessions/${encodeURIComponent(sessionId)}?${query}`,
      { method: 'DELETE', signal },
      [204],
    );
  }

  listAgentMessages(
    sessionId: string,
    startOrdinal: string,
    limit: string,
    signal?: AbortSignal,
  ): Promise<AgentMessageList> {
    const query = new URLSearchParams({ startOrdinal, limit });
    return this.#json(
      `/api/v1/agent/sessions/${encodeURIComponent(sessionId)}/messages?${query}`,
      { method: 'GET', signal },
      [200],
    );
  }

  async startAgentRun(
    request: StartAgentRunRequest,
    signal?: AbortSignal,
  ): Promise<AgentRunAccepted> {
    const payload = await this.#json<unknown>(
      '/api/v1/agent/runs',
      { method: 'POST', body: JSON.stringify(request), signal },
      [202],
    );
    if (!isAgentRunAccepted(payload)) {
      throw protocolError('The runtime returned an invalid agent run acknowledgement');
    }
    return payload;
  }

  async agentRunSnapshot(runId: string, signal?: AbortSignal): Promise<AgentRunSnapshot> {
    const payload = await this.#json<unknown>(
      `/api/v1/agent/runs/${encodeURIComponent(runId)}`,
      { method: 'GET', signal },
      [200],
    );
    if (!isAgentRunSnapshot(payload)) {
      throw protocolError('The runtime returned an invalid agent run snapshot');
    }
    return payload;
  }

  async cancelAgentRun(runId: string, signal?: AbortSignal): Promise<CancelAgentRunResponse> {
    const payload = await this.#json<unknown>(
      `/api/v1/agent/runs/${encodeURIComponent(runId)}/cancel`,
      { method: 'POST', signal },
      [200],
    );
    if (!isCancelAgentRunResponse(payload)) {
      throw protocolError('The runtime returned an invalid agent run cancellation response');
    }
    return payload;
  }

  async decideAgentPermission(
    permissionId: string,
    request: DecideAgentPermissionRequest,
    signal?: AbortSignal,
  ): Promise<AgentPermissionResponse> {
    const payload = await this.#json<unknown>(
      `/api/v1/agent/runs/${encodeURIComponent(request.runId)}/permissions/${encodeURIComponent(permissionId)}/decision`,
      { method: 'POST', body: JSON.stringify(request), signal },
      [200],
    );
    if (!isAgentPermissionResponse(payload)) {
      throw protocolError('The runtime returned an invalid agent permission response');
    }
    return payload;
  }

  async subscribeAgentRun(
    runId: string,
    options: AgentSubscriptionOptions,
  ): Promise<AgentSubscription> {
    const initialSequence = options.afterSequence === undefined
      ? undefined
      : parseAgentCursor(options.afterSequence);
    const controller = new AbortController();
    const callerAbort = () => controller.abort(options.signal?.reason);
    if (options.signal?.aborted) callerAbort();
    else options.signal?.addEventListener('abort', callerAbort, { once: true });

    const headers = new Headers({ Accept: 'text/event-stream' });
    if (options.afterSequence !== undefined) {
      headers.set('Last-Event-ID', options.afterSequence);
    }

    let response: Response;
    try {
      response = await this.#fetch(
        `${this.#baseUrl}/api/v1/agent/runs/${encodeURIComponent(runId)}/events`,
        { method: 'GET', headers, signal: controller.signal },
      );
    } catch (error) {
      options.signal?.removeEventListener('abort', callerAbort);
      throw error;
    }
    if (response.status !== 200) {
      options.signal?.removeEventListener('abort', callerAbort);
      throw await apiFailure(response);
    }
    const body = response.body;
    if (!body) {
      options.signal?.removeEventListener('abort', callerAbort);
      throw protocolError('The runtime opened an agent event stream without a body', response.status);
    }

    let active = true;
    let lastSequence = initialSequence;
    const parser = createParser({
      onEvent: (message) => {
        let payload: unknown;
        try {
          payload = JSON.parse(message.data);
        } catch {
          throw protocolError('The runtime emitted malformed agent event JSON');
        }
        if (message.event === 'error') {
          if (!isApiError(payload)) {
            throw protocolError('The runtime emitted an invalid agent stream error');
          }
          throw new ApiRequestError(payload);
        }
        if (!isAgentEventEnvelope(payload) || payload.runId !== runId) {
          throw protocolError('The runtime emitted an invalid agent event');
        }
        if ((message.id && message.id !== payload.sequence)
          || (message.event && message.event !== payload.event.type)) {
          throw protocolError('The runtime emitted inconsistent agent event metadata');
        }
        const sequence = BigInt(payload.sequence);
        if (lastSequence !== undefined && sequence <= lastSequence) {
          throw protocolError('The runtime emitted an out-of-order agent event');
        }
        lastSequence = sequence;
        if (active) options.onEvent(payload);
      },
    });

    void (async () => {
      const reader = body.getReader();
      const decoder = new TextDecoder();
      try {
        while (active) {
          const { done, value } = await reader.read();
          if (done) {
            parser.reset({ consume: true });
            break;
          }
          parser.feed(decoder.decode(value, { stream: true }));
        }
      } catch (error) {
        if (active && !isAbortError(error)) {
          options.onError?.(error instanceof Error ? error : protocolError('Agent stream failed'));
          controller.abort();
        }
      } finally {
        active = false;
        options.signal?.removeEventListener('abort', callerAbort);
        reader.releaseLock();
        options.onClose?.();
      }
    })();

    return {
      close: () => {
        if (!active) return;
        active = false;
        options.signal?.removeEventListener('abort', callerAbort);
        controller.abort();
      },
    };
  }

  startQuery(request: StartQueryRequest, signal?: AbortSignal): Promise<QueryAccepted> {
    return this.#json(
      '/api/v1/queries',
      { method: 'POST', body: JSON.stringify(request), signal },
      [202],
    );
  }

  operationSnapshot(operationId: string, signal?: AbortSignal): Promise<OperationSnapshot> {
    return this.#json(
      `/api/v1/operations/${encodeURIComponent(operationId)}`,
      { method: 'GET', signal },
      [200],
    );
  }

  cancelOperation(operationId: string, signal?: AbortSignal): Promise<CancelOperationResponse> {
    return this.#json(
      `/api/v1/operations/${encodeURIComponent(operationId)}/cancel`,
      { method: 'POST', signal },
      [200],
    );
  }

  async subscribeOperation(
    operationId: string,
    options: OperationSubscriptionOptions,
  ): Promise<OperationSubscription> {
    const initialSequence = options.afterSequence === undefined
      ? undefined
      : parseOperationCursor(options.afterSequence);
    const controller = new AbortController();
    const callerAbort = () => controller.abort(options.signal?.reason);
    if (options.signal?.aborted) callerAbort();
    else options.signal?.addEventListener('abort', callerAbort, { once: true });

    const headers = new Headers({ Accept: 'text/event-stream' });
    if (options.afterSequence !== undefined) {
      headers.set('Last-Event-ID', options.afterSequence);
    }

    let response: Response;
    try {
      response = await this.#fetch(
        `${this.#baseUrl}/api/v1/operations/${encodeURIComponent(operationId)}/events`,
        { method: 'GET', headers, signal: controller.signal },
      );
    } catch (error) {
      options.signal?.removeEventListener('abort', callerAbort);
      throw error;
    }
    if (response.status !== 200) {
      options.signal?.removeEventListener('abort', callerAbort);
      throw await apiFailure(response);
    }
    const body = response.body;
    if (!body) {
      options.signal?.removeEventListener('abort', callerAbort);
      throw protocolError('The runtime opened an event stream without a body', response.status);
    }

    let active = true;
    let lastSequence = initialSequence;
    const parser = createParser({
      onEvent: (message) => {
        let payload: unknown;
        try {
          payload = JSON.parse(message.data);
        } catch {
          throw protocolError('The runtime emitted malformed operation event JSON');
        }
        if (message.event === 'error') {
          if (!isApiError(payload)) {
            throw protocolError('The runtime emitted an invalid stream error');
          }
          throw new ApiRequestError(payload);
        }
        if (!isOperationEventEnvelope(payload) || payload.operationId !== operationId) {
          throw protocolError('The runtime emitted an invalid operation event');
        }
        if ((message.id && message.id !== payload.sequence)
          || (message.event && message.event !== payload.event.type)) {
          throw protocolError('The runtime emitted inconsistent operation event metadata');
        }
        const sequence = BigInt(payload.sequence);
        if (lastSequence !== undefined && sequence <= lastSequence) {
          throw protocolError('The runtime emitted an out-of-order operation event');
        }
        lastSequence = sequence;
        if (active) options.onEvent(payload);
      },
    });

    void (async () => {
      const reader = body.getReader();
      const decoder = new TextDecoder();
      try {
        while (active) {
          const { done, value } = await reader.read();
          if (done) {
            parser.reset({ consume: true });
            break;
          }
          parser.feed(decoder.decode(value, { stream: true }));
        }
      } catch (error) {
        if (active && !isAbortError(error)) {
          options.onError?.(error instanceof Error ? error : protocolError('Operation stream failed'));
          controller.abort();
        }
      } finally {
        active = false;
        options.signal?.removeEventListener('abort', callerAbort);
        reader.releaseLock();
        options.onClose?.();
      }
    })();

    return {
      close: () => {
        if (!active) return;
        active = false;
        options.signal?.removeEventListener('abort', callerAbort);
        controller.abort();
      },
    };
  }

  resultPage(
    resultId: string,
    request: ResultPageRequest,
    signal?: AbortSignal,
  ): Promise<ResultPage> {
    const query = new URLSearchParams({
      offset: request.offset,
      maxRows: request.maxRows,
      maxBytes: request.maxBytes,
    });
    return this.#json(
      `/api/v1/results/${encodeURIComponent(resultId)}?${query}`,
      { method: 'GET', signal },
      [200],
    );
  }
}
