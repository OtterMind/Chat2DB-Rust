import { describe, expect, it, vi } from 'vitest';

import { ApiRequestError, OperationEventEnvelope } from './client';
import { HttpBackendClient } from './http';

function jsonResponse(value: unknown, status: number): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function eventStream(events: OperationEventEnvelope[]): Response {
  const encoder = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const event of events) {
        controller.enqueue(encoder.encode(`id: ${event.sequence}\ndata: ${JSON.stringify(event)}\n\n`));
      }
      controller.close();
    },
  });
  return new Response(stream, {
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
  });
}

describe('HttpBackendClient', () => {
  it('preserves the stable ApiError envelope and HTTP status', async () => {
    const fetch = vi.fn(async () => jsonResponse({
      code: 'revision_conflict',
      message: 'Datasource revision changed',
      retryable: false,
      details: {
        type: 'revision_conflict',
        expectedRevision: '4',
        actualRevision: '5',
      },
    }, 409));
    const client = new HttpBackendClient({ fetch });

    const error = await client.updateDatasource('source/1', {
      name: 'Warehouse',
      driverId: 'postgresql',
      expectedRevision: '4',
      secretChange: { action: 'keep' },
    }).catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(ApiRequestError);
    expect(error).toMatchObject({
      status: 409,
      apiError: {
        code: 'revision_conflict',
        details: { actualRevision: '5' },
      },
    });
    expect(fetch).toHaveBeenCalledWith(
      '/api/v1/datasources/source%2F1',
      expect.objectContaining({ method: 'PUT' }),
    );
  });

  it('rejects an unstructured HTTP failure', async () => {
    const client = new HttpBackendClient({
      fetch: async () => jsonResponse({ message: 'raw internal error' }, 500),
    });

    await expect(client.listDatasources()).rejects.toMatchObject({
      apiError: { code: 'invalid_transport_response' },
      status: 500,
    });
  });

  it('replays SSE events in sequence and sends Last-Event-ID', async () => {
    const events: OperationEventEnvelope[] = [
      {
        operationId: 'operation-1',
        sequence: '8',
        occurredAtMs: '1740000000000',
        event: { type: 'started' },
      },
      {
        operationId: 'operation-1',
        sequence: '9',
        occurredAtMs: '1740000000001',
        event: { type: 'progress', rowCount: '32', byteCount: '4096' },
      },
    ];
    let request: RequestInit | undefined;
    const fetch = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      request = init;
      return eventStream(events);
    });
    const client = new HttpBackendClient({ fetch });
    const received: OperationEventEnvelope[] = [];
    let closeStream: (() => void) | undefined;
    const closed = new Promise<void>((resolve) => { closeStream = resolve; });

    await client.subscribeOperation('operation-1', {
      afterSequence: '7',
      onEvent: (event) => received.push(event),
      onClose: () => closeStream?.(),
    });
    await closed;

    expect(new Headers(request?.headers).get('Last-Event-ID')).toBe('7');
    expect(received.map((event) => event.sequence)).toEqual(['8', '9']);
    expect(received[1]?.event).toEqual({ type: 'progress', rowCount: '32', byteCount: '4096' });
  });

  it('reports an out-of-order event instead of delivering it', async () => {
    const events: OperationEventEnvelope[] = [
      {
        operationId: 'operation-1',
        sequence: '8',
        occurredAtMs: '1740000000000',
        event: { type: 'started' },
      },
      {
        operationId: 'operation-1',
        sequence: '8',
        occurredAtMs: '1740000000001',
        event: { type: 'cancelled', reason: null },
      },
    ];
    const client = new HttpBackendClient({ fetch: async () => eventStream(events) });
    const received: OperationEventEnvelope[] = [];
    let reportError: ((error: Error) => void) | undefined;
    const failed = new Promise<Error>((resolve) => { reportError = resolve; });

    await client.subscribeOperation('operation-1', {
      onEvent: (event) => received.push(event),
      onError: (error) => reportError?.(error),
    });

    await expect(failed).resolves.toMatchObject({
      apiError: { code: 'invalid_transport_response' },
    });
    expect(received.map((event) => event.sequence)).toEqual(['8']);
  });

  it('preserves a structured ApiError sent through the SSE stream', async () => {
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(
          'event: error\ndata: {"code":"replay_window_exceeded","message":"Replay expired","retryable":true,"details":{"type":"replay_window","requestedSequence":"1","oldestAvailableSequence":"4","latestSequence":"9"}}\n\n',
        ));
        controller.close();
      },
    });
    const client = new HttpBackendClient({
      fetch: async () => new Response(stream, { status: 200 }),
    });
    let reportError: ((error: Error) => void) | undefined;
    const failed = new Promise<Error>((resolve) => { reportError = resolve; });

    await client.subscribeOperation('operation-1', {
      onEvent: () => undefined,
      onError: (error) => reportError?.(error),
    });

    await expect(failed).resolves.toMatchObject({
      apiError: {
        code: 'replay_window_exceeded',
        details: { requestedSequence: '1', oldestAvailableSequence: '4' },
      },
    });
  });

  it('closes an HTTP subscription locally without calling cancel', async () => {
    let streamController: ReadableStreamDefaultController<Uint8Array> | undefined;
    const fetch = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const stream = new ReadableStream<Uint8Array>({
        start(controller) {
          streamController = controller;
          init?.signal?.addEventListener('abort', () => {
            controller.error(new DOMException('Aborted', 'AbortError'));
          }, { once: true });
        },
      });
      return new Response(stream, { status: 200 });
    });
    const client = new HttpBackendClient({ fetch });
    let closeStream: (() => void) | undefined;
    const closed = new Promise<void>((resolve) => { closeStream = resolve; });
    const subscription = await client.subscribeOperation('operation-1', {
      onEvent: () => undefined,
      onClose: () => closeStream?.(),
    });

    subscription.close();
    await closed;

    expect(streamController).toBeDefined();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(String(fetch.mock.calls[0]?.[0])).toContain('/events');
    expect(String(fetch.mock.calls[0]?.[0])).not.toContain('/cancel');
  });

  it.each(['invalid', '-1', '18446744073709551616'])(
    'returns the canonical cursor error for malformed replay cursor %s',
    async (afterSequence) => {
      const fetch = vi.fn(async () => eventStream([]));
      const client = new HttpBackendClient({ fetch });

      await expect(client.subscribeOperation('operation-1', {
        afterSequence,
        onEvent: () => undefined,
      })).rejects.toMatchObject({
        apiError: { code: 'invalid_last_event_id' },
      });
      expect(fetch).not.toHaveBeenCalled();
    },
  );
});
