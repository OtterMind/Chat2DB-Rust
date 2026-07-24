import { Channel, invoke } from '@tauri-apps/api/core';

import type {
  AgentMessageList,
  AgentSession,
  AgentSessionList,
  BackendClient,
  CancelOperationResponse,
  CreateAgentSessionRequest,
  CreateDatasourceRequest,
  CreateProviderProfileRequest,
  Datasource,
  DatasourceList,
  HealthResponse,
  OperationEventEnvelope,
  OperationSnapshot,
  OperationStreamMessage,
  OperationSubscription,
  OperationSubscriptionAccepted,
  OperationSubscriptionOptions,
  ProviderProfile,
  ProviderProfileList,
  QueryAccepted,
  ResultPage,
  ResultPageRequest,
  StartQueryRequest,
  UpdateAgentSessionRequest,
  UpdateDatasourceRequest,
  UpdateProviderProfileRequest,
} from './client';
import {
  abortError,
  isOperationStreamMessage,
  isOperationSubscriptionAccepted,
  normalizeApiError,
  parseOperationCursor,
  protocolError,
  withAbort,
} from './client';

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

interface EventChannel<T> {
  onmessage: (message: T) => void;
}

interface TauriBackendOptions {
  invoke?: Invoke;
  channel?: <T>() => EventChannel<T>;
}

export class TauriBackendClient implements BackendClient {
  readonly transport = 'tauri' as const;
  readonly #invoke: Invoke;
  readonly #channel: <T>() => EventChannel<T>;

  constructor(options: TauriBackendOptions = {}) {
    this.#invoke = options.invoke ?? invoke;
    this.#channel = options.channel ?? (() => new Channel());
  }

  async #request<T>(command: string, args?: Record<string, unknown>, signal?: AbortSignal): Promise<T> {
    try {
      return await withAbort(this.#invoke<T>(command, args), signal);
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') throw error;
      throw normalizeApiError(error);
    }
  }

  health(signal?: AbortSignal): Promise<HealthResponse> {
    return this.#request('health', undefined, signal);
  }

  listDatasources(signal?: AbortSignal): Promise<DatasourceList> {
    return this.#request('list_datasources', undefined, signal);
  }

  createDatasource(request: CreateDatasourceRequest, signal?: AbortSignal): Promise<Datasource> {
    return this.#request('create_datasource', { request }, signal);
  }

  getDatasource(datasourceId: string, signal?: AbortSignal): Promise<Datasource> {
    return this.#request('get_datasource', { datasourceId }, signal);
  }

  updateDatasource(
    datasourceId: string,
    request: UpdateDatasourceRequest,
    signal?: AbortSignal,
  ): Promise<Datasource> {
    return this.#request('update_datasource', { datasourceId, request }, signal);
  }

  deleteDatasource(
    datasourceId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.#request('delete_datasource', { datasourceId, expectedRevision }, signal);
  }

  listProviderProfiles(signal?: AbortSignal): Promise<ProviderProfileList> {
    return this.#request('list_provider_profiles', undefined, signal);
  }

  createProviderProfile(
    request: CreateProviderProfileRequest,
    signal?: AbortSignal,
  ): Promise<ProviderProfile> {
    return this.#request('create_provider_profile', { request }, signal);
  }

  getProviderProfile(providerId: string, signal?: AbortSignal): Promise<ProviderProfile> {
    return this.#request('get_provider_profile', { providerId }, signal);
  }

  updateProviderProfile(
    providerId: string,
    request: UpdateProviderProfileRequest,
    signal?: AbortSignal,
  ): Promise<ProviderProfile> {
    return this.#request('update_provider_profile', { providerId, request }, signal);
  }

  deleteProviderProfile(
    providerId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.#request('delete_provider_profile', { providerId, expectedRevision }, signal);
  }

  listAgentSessions(signal?: AbortSignal): Promise<AgentSessionList> {
    return this.#request('list_agent_sessions', undefined, signal);
  }

  createAgentSession(
    request: CreateAgentSessionRequest,
    signal?: AbortSignal,
  ): Promise<AgentSession> {
    return this.#request('create_agent_session', { request }, signal);
  }

  getAgentSession(sessionId: string, signal?: AbortSignal): Promise<AgentSession> {
    return this.#request('get_agent_session', { sessionId }, signal);
  }

  updateAgentSession(
    sessionId: string,
    request: UpdateAgentSessionRequest,
    signal?: AbortSignal,
  ): Promise<AgentSession> {
    return this.#request('update_agent_session', { sessionId, request }, signal);
  }

  deleteAgentSession(
    sessionId: string,
    expectedRevision: string,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.#request('delete_agent_session', { sessionId, expectedRevision }, signal);
  }

  listAgentMessages(
    sessionId: string,
    startOrdinal: string,
    limit: string,
    signal?: AbortSignal,
  ): Promise<AgentMessageList> {
    return this.#request(
      'list_agent_messages',
      { sessionId, startOrdinal, limit },
      signal,
    );
  }

  startQuery(request: StartQueryRequest, signal?: AbortSignal): Promise<QueryAccepted> {
    return this.#request('start_query', { request }, signal);
  }

  operationSnapshot(operationId: string, signal?: AbortSignal): Promise<OperationSnapshot> {
    return this.#request('operation_snapshot', { operationId }, signal);
  }

  cancelOperation(operationId: string, signal?: AbortSignal): Promise<CancelOperationResponse> {
    return this.#request('cancel_operation', { operationId }, signal);
  }

  async subscribeOperation(
    operationId: string,
    options: OperationSubscriptionOptions,
  ): Promise<OperationSubscription> {
    let active = true;
    let subscriptionId: string | undefined;
    let lastSequence = options.afterSequence === undefined
      ? undefined
      : parseOperationCursor(options.afterSequence);
    const channel = this.#channel<OperationStreamMessage>();
    const unsubscribe = () => {
      if (subscriptionId === undefined) return;
      const releasedId = subscriptionId;
      subscriptionId = undefined;
      void this.#request<void>('unsubscribe_operation', { subscriptionId: releasedId }).catch(
        () => undefined,
      );
    };
    const close = () => {
      if (!active) return;
      active = false;
      channel.onmessage = () => undefined;
      options.signal?.removeEventListener('abort', close);
      unsubscribe();
      options.onClose?.();
    };
    const fail = (error: Error) => {
      if (!active) return;
      options.onError?.(error);
      close();
    };
    channel.onmessage = (message) => {
      if (!active) return;
      if (!isOperationStreamMessage(message)) {
        fail(protocolError('The desktop runtime emitted an invalid operation stream message'));
        return;
      }
      if (message.type === 'error') {
        fail(normalizeApiError(message.error));
        return;
      }
      if (message.type === 'end') {
        close();
        return;
      }
      const event: OperationEventEnvelope = message.event;
      if (event.operationId !== operationId) {
        fail(protocolError('The desktop runtime emitted an event for another operation'));
        return;
      }
      const sequence = BigInt(event.sequence);
      if (lastSequence !== undefined && sequence <= lastSequence) {
        fail(protocolError('The desktop runtime emitted an out-of-order operation event'));
        return;
      }
      lastSequence = sequence;
      options.onEvent(event);
    };
    if (options.signal?.aborted) {
      close();
      throw abortError(options.signal);
    }
    options.signal?.addEventListener('abort', close, { once: true });

    try {
      const accepted = await this.#request<OperationSubscriptionAccepted>(
        'subscribe_operation',
        {
          operationId,
          afterSequence: options.afterSequence ?? null,
          onEvent: channel,
        },
      );
      if (!isOperationSubscriptionAccepted(accepted)) {
        throw protocolError('The desktop runtime returned an invalid subscription id');
      }
      subscriptionId = accepted.subscriptionId;
      if (!active || options.signal?.aborted) {
        unsubscribe();
        if (options.signal?.aborted) throw abortError(options.signal);
      }
    } catch (error) {
      close();
      throw error;
    }

    return { close };
  }

  resultPage(
    resultId: string,
    request: ResultPageRequest,
    signal?: AbortSignal,
  ): Promise<ResultPage> {
    return this.#request('result_page', { resultId, request }, signal);
  }
}
