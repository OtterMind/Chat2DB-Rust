import { createParser } from 'eventsource-parser';

import type {
  BackendClient,
  CancelOperationResponse,
  CreateDatasourceRequest,
  Datasource,
  DatasourceList,
  HealthResponse,
  OperationSnapshot,
  OperationSubscription,
  OperationSubscriptionOptions,
  QueryAccepted,
  ResultPage,
  ResultPageRequest,
  StartQueryRequest,
  UpdateDatasourceRequest,
} from './client';
import {
  ApiRequestError,
  isApiError,
  isOperationEventEnvelope,
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
