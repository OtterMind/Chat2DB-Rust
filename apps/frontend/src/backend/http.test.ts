import { describe, expect, it, vi } from 'vitest';

import type {
  AgentEventEnvelope,
  AgentRunSnapshot,
  CreateAgentSessionRequest,
  CreateProviderProfileRequest,
  DecideAgentPermissionRequest,
  OperationEventEnvelope,
  StartAgentRunRequest,
  UpdateAgentSessionRequest,
  UpdateProviderProfileRequest,
} from './client';
import { ApiRequestError } from './client';
import { HttpBackendClient } from './http';

const createProviderRequest = {
  name: 'Primary',
  kind: 'open_ai_compatible',
  baseUrl: 'https://models.example/v1',
  model: 'model-1',
  contextWindowTokens: '18446744073709551615',
  maxOutputTokens: '32768',
  credentials: { apiKey: 'provider-secret' },
} satisfies CreateProviderProfileRequest;

const updateProviderRequest = {
  expectedRevision: '18446744073709551614',
  name: 'Primary updated',
  kind: 'open_ai_compatible',
  baseUrl: 'https://models.example/v1',
  model: 'model-2',
  contextWindowTokens: '18446744073709551615',
  maxOutputTokens: '65536',
  secretChange: { action: 'keep' },
} satisfies UpdateProviderProfileRequest;

const providerProfile = {
  id: 'provider/1',
  name: 'Primary',
  kind: 'open_ai_compatible',
  baseUrl: 'https://models.example/v1',
  model: 'model-1',
  contextWindowTokens: '18446744073709551615',
  maxOutputTokens: '32768',
  hasSecret: true,
  revision: '18446744073709551614',
  createdAtMs: '1740000000000',
  updatedAtMs: '1740000000001',
};

const createSessionRequest = {
  title: 'Investigate orders',
  providerId: 'provider/1',
  datasourceId: 'source/1',
  systemPrompt: 'Use read-only SQL.',
} satisfies CreateAgentSessionRequest;

const updateSessionRequest = {
  expectedRevision: '18446744073709551613',
  title: 'Investigate recent orders',
  providerId: 'provider/1',
  datasourceId: 'source/1',
} satisfies UpdateAgentSessionRequest;

const agentSession = {
  id: 'session/1',
  title: 'Investigate orders',
  providerId: 'provider/1',
  datasourceId: 'source/1',
  revision: '18446744073709551613',
  createdAtMs: '1740000000002',
  updatedAtMs: '1740000000003',
};

const startRunRequest = {
  sessionId: agentSession.id,
  message: 'Find the largest recent orders',
  sqlPermissionMode: 'read_only',
} satisfies StartAgentRunRequest;

const agentRunSnapshot = {
  runId: 'run/1',
  sessionId: agentSession.id,
  status: 'waiting_for_permission',
  lastSequence: '18446744073709551614',
  startedAtMs: '1740000000010',
  updatedAtMs: '1740000000011',
  modelRounds: '2',
  toolCalls: '1',
  usage: { inputTokens: '120', outputTokens: '40', totalTokens: '160' },
  pendingPermission: {
    permissionId: 'permission-1',
    runId: 'run/1',
    toolCallId: 'tool-1',
    toolName: 'execute_sql',
    argumentsSha256: 'a'.repeat(64),
    summary: 'Update one order',
    requestedAtMs: '1740000000011',
    expiresAtMs: '1740000060011',
  },
} satisfies AgentRunSnapshot;

const permissionDecisionRequest = {
  runId: 'run/1',
  toolCallId: 'tool-1',
  decision: 'allow_once',
  argumentsSha256: 'a'.repeat(64),
} satisfies DecideAgentPermissionRequest;

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

function agentEventStream(events: AgentEventEnvelope[]): Response {
  const encoder = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const event of events) {
        controller.enqueue(encoder.encode(
          `id: ${event.sequence}\nevent: ${event.event.type}\ndata: ${JSON.stringify(event)}\n\n`,
        ));
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
  it('loads the managed driver inventory from the HTTP contract', async () => {
    const inventory = {
      items: [{
        packId: 'h2',
        name: 'H2',
        version: '2.3.232',
        driverId: 'sha256:driver',
        driverClass: 'org.h2.Driver',
        artifactCount: 1,
        artifactBytes: '2614933',
      }],
    };
    const fetch = vi.fn(async () => jsonResponse(inventory, 200));
    const client = new HttpBackendClient({ baseUrl: 'http://127.0.0.1:10825/', fetch });

    await expect(client.listDrivers()).resolves.toEqual(inventory);
    expect(fetch).toHaveBeenCalledWith(
      'http://127.0.0.1:10825/api/v1/drivers',
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('maps every agent catalog method to its HTTP contract without narrowing integers', async () => {
    const updatedProvider = {
      ...providerProfile,
      name: updateProviderRequest.name,
      model: updateProviderRequest.model,
      revision: '18446744073709551615',
    };
    const updatedSession = {
      ...agentSession,
      title: updateSessionRequest.title,
      revision: '18446744073709551614',
    };
    const messageList = {
      items: [{
        id: 'message-1',
        sessionId: agentSession.id,
        role: 'user',
        content: [{ type: 'text', text: 'Show recent orders' }],
        ordinal: '18446744073709551615',
        createdAtMs: '1740000000004',
      }],
      hasMore: false,
    };
    const responses = [
      jsonResponse({ items: [providerProfile] }, 200),
      jsonResponse(providerProfile, 201),
      jsonResponse(providerProfile, 200),
      jsonResponse(updatedProvider, 200),
      new Response(null, { status: 204 }),
      jsonResponse({ items: [agentSession] }, 200),
      jsonResponse(agentSession, 201),
      jsonResponse(agentSession, 200),
      jsonResponse(updatedSession, 200),
      new Response(null, { status: 204 }),
      jsonResponse(messageList, 200),
    ];
    const calls: Array<{ input: string; init?: RequestInit }> = [];
    const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input: String(input), init });
      const response = responses.shift();
      if (!response) throw new Error('unexpected request');
      return response;
    });
    const client = new HttpBackendClient({ baseUrl: 'http://127.0.0.1:10825/', fetch });

    await client.listProviderProfiles();
    const createdProvider = await client.createProviderProfile(createProviderRequest);
    await client.getProviderProfile(providerProfile.id);
    await client.updateProviderProfile(providerProfile.id, updateProviderRequest);
    await client.deleteProviderProfile(providerProfile.id, providerProfile.revision);
    await client.listAgentSessions();
    await client.createAgentSession(createSessionRequest);
    await client.getAgentSession(agentSession.id);
    await client.updateAgentSession(agentSession.id, updateSessionRequest);
    await client.deleteAgentSession(agentSession.id, agentSession.revision);
    const messages = await client.listAgentMessages(
      agentSession.id,
      '18446744073709551615',
      '4294967295',
    );

    expect(createdProvider.contextWindowTokens).toBe('18446744073709551615');
    expect(messages.items[0]?.ordinal).toBe('18446744073709551615');
    expect(calls.map(({ input, init }) => ({
      input,
      method: init?.method,
      body: init?.body,
    }))).toEqual([
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/providers',
        method: 'GET',
        body: undefined,
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/providers',
        method: 'POST',
        body: JSON.stringify(createProviderRequest),
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/providers/provider%2F1',
        method: 'GET',
        body: undefined,
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/providers/provider%2F1',
        method: 'PUT',
        body: JSON.stringify(updateProviderRequest),
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/providers/provider%2F1?expectedRevision=18446744073709551614',
        method: 'DELETE',
        body: undefined,
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/sessions',
        method: 'GET',
        body: undefined,
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/sessions',
        method: 'POST',
        body: JSON.stringify(createSessionRequest),
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/sessions/session%2F1',
        method: 'GET',
        body: undefined,
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/sessions/session%2F1',
        method: 'PUT',
        body: JSON.stringify(updateSessionRequest),
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/sessions/session%2F1?expectedRevision=18446744073709551613',
        method: 'DELETE',
        body: undefined,
      },
      {
        input: 'http://127.0.0.1:10825/api/v1/agent/sessions/session%2F1/messages?startOrdinal=18446744073709551615&limit=4294967295',
        method: 'GET',
        body: undefined,
      },
    ]);
  });

  it('requires 201 for agent catalog creates and 204 for deletes', async () => {
    const failure = {
      code: 'unexpected_status',
      message: 'The route returned the wrong status',
      retryable: false,
    };
    const client = new HttpBackendClient({
      fetch: async () => jsonResponse(failure, 200),
    });

    await expect(client.createProviderProfile(createProviderRequest)).rejects.toMatchObject({
      status: 200,
      apiError: { code: 'unexpected_status' },
    });
    await expect(client.createAgentSession(createSessionRequest)).rejects.toMatchObject({
      status: 200,
      apiError: { code: 'unexpected_status' },
    });
    await expect(client.deleteProviderProfile('provider-1', '9')).rejects.toMatchObject({
      status: 200,
      apiError: { code: 'unexpected_status' },
    });
    await expect(client.deleteAgentSession('session-1', '9')).rejects.toMatchObject({
      status: 200,
      apiError: { code: 'unexpected_status' },
    });
  });

  it('maps the agent run lifecycle to the HTTP contract and validates responses', async () => {
    const responses = [
      jsonResponse({ runId: 'run/1', sessionId: agentSession.id }, 202),
      jsonResponse(agentRunSnapshot, 200),
      jsonResponse({ permissionId: 'permission/1', status: 'approved' }, 200),
      jsonResponse({ runId: 'run/1', disposition: 'accepted' }, 200),
    ];
    const calls: Array<{ input: string; init?: RequestInit }> = [];
    const client = new HttpBackendClient({
      baseUrl: 'http://127.0.0.1:10825/',
      fetch: async (input, init) => {
        calls.push({ input: String(input), init });
        const response = responses.shift();
        if (!response) throw new Error('unexpected request');
        return response;
      },
    });

    await expect(client.startAgentRun(startRunRequest)).resolves.toEqual({
      runId: 'run/1',
      sessionId: agentSession.id,
    });
    await expect(client.agentRunSnapshot('run/1')).resolves.toEqual(agentRunSnapshot);
    await expect(
      client.decideAgentPermission('permission/1', permissionDecisionRequest),
    ).resolves.toEqual({ permissionId: 'permission/1', status: 'approved' });
    await expect(client.cancelAgentRun('run/1')).resolves.toEqual({
      runId: 'run/1',
      disposition: 'accepted',
    });

    expect(calls.map(({ input, init }) => ({ input, method: init?.method, body: init?.body })))
      .toEqual([
        {
          input: 'http://127.0.0.1:10825/api/v1/agent/runs',
          method: 'POST',
          body: JSON.stringify(startRunRequest),
        },
        {
          input: 'http://127.0.0.1:10825/api/v1/agent/runs/run%2F1',
          method: 'GET',
          body: undefined,
        },
        {
          input: 'http://127.0.0.1:10825/api/v1/agent/runs/run%2F1/permissions/permission%2F1/decision',
          method: 'POST',
          body: JSON.stringify(permissionDecisionRequest),
        },
        {
          input: 'http://127.0.0.1:10825/api/v1/agent/runs/run%2F1/cancel',
          method: 'POST',
          body: undefined,
        },
      ]);
  });

  it('rejects an invalid successful agent run response', async () => {
    const client = new HttpBackendClient({
      fetch: async () => jsonResponse({ runId: 7, sessionId: 'session-1' }, 202),
    });

    await expect(client.startAgentRun(startRunRequest)).rejects.toMatchObject({
      apiError: { code: 'invalid_transport_response' },
    });
  });

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

  it('streams typed agent events in sequence and sends the run replay cursor', async () => {
    const events: AgentEventEnvelope[] = [
      {
        runId: 'run/1',
        sequence: '8',
        occurredAtMs: '1740000000020',
        event: { type: 'text_delta', delta: 'Found ' },
      },
      {
        runId: 'run/1',
        sequence: '9',
        occurredAtMs: '1740000000021',
        event: {
          type: 'tool_completed',
          toolCallId: 'tool-1',
          name: 'execute_sql',
          output: {
            type: 'result',
            handle: {
              handleId: 'result-1',
              rowCount: '18446744073709551615',
              byteCount: '1024',
              truncatedByMaxRows: true,
              truncatedByMaxResultBytes: false,
              createdAtMs: '1740000000021',
              expiresAtMs: '1740000060021',
              columns: [{
                jdbcType: 12,
                jdbcTypeName: 'VARCHAR',
                label: 'name',
                name: 'name',
                nullability: 'nullable',
                ordinal: 1,
                valueType: 'text',
              }],
              columnsTruncated: false,
              sampleRows: [{ values: [{ type: 'text', value: 'order-1' }] }],
              sampleTruncated: true,
            },
          },
        },
      },
      {
        runId: 'run/1',
        sequence: '10',
        occurredAtMs: '1740000000022',
        event: { type: 'completed', messageId: 'message-2' },
      },
    ];
    let input = '';
    let request: RequestInit | undefined;
    const client = new HttpBackendClient({
      fetch: async (nextInput, init) => {
        input = String(nextInput);
        request = init;
        return agentEventStream(events);
      },
    });
    const received: AgentEventEnvelope[] = [];
    let closeStream: (() => void) | undefined;
    const closed = new Promise<void>((resolve) => { closeStream = resolve; });

    await client.subscribeAgentRun('run/1', {
      afterSequence: '7',
      onEvent: (event) => received.push(event),
      onClose: () => closeStream?.(),
    });
    await closed;

    expect(input).toBe('/api/v1/agent/runs/run%2F1/events');
    expect(new Headers(request?.headers).get('Last-Event-ID')).toBe('7');
    expect(received.map((event) => event.sequence)).toEqual(['8', '9', '10']);
    expect(received[1]?.event).toMatchObject({
      type: 'tool_completed',
      output: { type: 'result', handle: { rowCount: '18446744073709551615' } },
    });
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
