import { describe, expect, it, vi } from 'vitest';

import type {
  AgentRunSnapshot,
  AgentStreamMessage,
  CreateAgentSessionRequest,
  CreateProviderProfileRequest,
  DecideAgentPermissionRequest,
  OperationStreamMessage,
  StartAgentRunRequest,
  UpdateAgentSessionRequest,
  UpdateProviderProfileRequest,
} from './client';
import { ApiRequestError } from './client';
import { TauriBackendClient } from './tauri';

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

const startRunRequest = {
  sessionId: 'session/1',
  message: 'Summarize recent orders',
  sqlPermissionMode: 'read_only',
} satisfies StartAgentRunRequest;

const agentRunSnapshot = {
  runId: 'run/1',
  sessionId: 'session/1',
  status: 'running',
  lastSequence: '5',
  startedAtMs: '1740000000010',
  updatedAtMs: '1740000000011',
  modelRounds: '1',
  toolCalls: '0',
  usage: { inputTokens: '100', outputTokens: '20', totalTokens: '120' },
} satisfies AgentRunSnapshot;

const permissionDecisionRequest = {
  runId: 'run/1',
  toolCallId: 'tool-1',
  decision: 'allow_once',
  argumentsSha256: 'a'.repeat(64),
} satisfies DecideAgentPermissionRequest;

describe('TauriBackendClient', () => {
  it('maps datasource calls to camel-case Tauri arguments', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ command, args });
      return undefined as T;
    };
    const client = new TauriBackendClient({ invoke });

    await client.deleteDatasource('source-1', '12');

    expect(calls).toEqual([{
      command: 'delete_datasource',
      args: { datasourceId: 'source-1', expectedRevision: '12' },
    }]);
  });

  it('maps every agent catalog method to snake-case commands and camel-case arguments', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ command, args });
      return undefined as T;
    };
    const client = new TauriBackendClient({ invoke });

    await client.listProviderProfiles();
    await client.createProviderProfile(createProviderRequest);
    await client.getProviderProfile('provider/1');
    await client.updateProviderProfile('provider/1', updateProviderRequest);
    await client.deleteProviderProfile('provider/1', '18446744073709551614');
    await client.listAgentSessions();
    await client.createAgentSession(createSessionRequest);
    await client.getAgentSession('session/1');
    await client.updateAgentSession('session/1', updateSessionRequest);
    await client.deleteAgentSession('session/1', '18446744073709551613');
    await client.listAgentMessages(
      'session/1',
      '18446744073709551615',
      '4294967295',
    );

    expect(calls).toEqual([
      { command: 'list_provider_profiles', args: undefined },
      { command: 'create_provider_profile', args: { request: createProviderRequest } },
      { command: 'get_provider_profile', args: { providerId: 'provider/1' } },
      {
        command: 'update_provider_profile',
        args: { providerId: 'provider/1', request: updateProviderRequest },
      },
      {
        command: 'delete_provider_profile',
        args: { providerId: 'provider/1', expectedRevision: '18446744073709551614' },
      },
      { command: 'list_agent_sessions', args: undefined },
      { command: 'create_agent_session', args: { request: createSessionRequest } },
      { command: 'get_agent_session', args: { sessionId: 'session/1' } },
      {
        command: 'update_agent_session',
        args: { sessionId: 'session/1', request: updateSessionRequest },
      },
      {
        command: 'delete_agent_session',
        args: { sessionId: 'session/1', expectedRevision: '18446744073709551613' },
      },
      {
        command: 'list_agent_messages',
        args: {
          sessionId: 'session/1',
          startOrdinal: '18446744073709551615',
          limit: '4294967295',
        },
      },
    ]);
  });

  it('normalizes an agent catalog command failure into ApiRequestError', async () => {
    const client = new TauriBackendClient({
      invoke: async () => Promise.reject({
        code: 'provider_not_found',
        message: 'The provider profile does not exist',
        retryable: false,
      }),
    });

    const error = await client.getProviderProfile('missing').catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiRequestError);
    expect(error).toMatchObject({
      apiError: { code: 'provider_not_found', message: 'The provider profile does not exist' },
    });
  });

  it('maps the agent run lifecycle to snake-case commands and validates responses', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ command, args });
      if (command === 'start_agent_run') {
        return { runId: 'run/1', sessionId: 'session/1' } as T;
      }
      if (command === 'agent_run_snapshot') return agentRunSnapshot as T;
      if (command === 'decide_agent_permission') {
        return { permissionId: 'permission/1', status: 'approved' } as T;
      }
      if (command === 'cancel_agent_run') {
        return { runId: 'run/1', disposition: 'accepted' } as T;
      }
      return undefined as T;
    };
    const client = new TauriBackendClient({ invoke });

    await client.startAgentRun(startRunRequest);
    await client.agentRunSnapshot('run/1');
    await client.decideAgentPermission('permission/1', permissionDecisionRequest);
    await client.cancelAgentRun('run/1');

    expect(calls).toEqual([
      { command: 'start_agent_run', args: { request: startRunRequest } },
      { command: 'agent_run_snapshot', args: { runId: 'run/1' } },
      {
        command: 'decide_agent_permission',
        args: { permissionId: 'permission/1', request: permissionDecisionRequest },
      },
      { command: 'cancel_agent_run', args: { runId: 'run/1' } },
    ]);
  });

  it('keeps agent Channel delivery ordered and releases only the subscription', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ command, args });
      return command === 'subscribe_agent_run'
        ? { subscriptionId: 'agent-subscription-1' } as T
        : undefined as T;
    };
    const channel: { onmessage: (event: AgentStreamMessage) => void } = {
      onmessage: () => undefined,
    };
    const client = new TauriBackendClient({
      invoke,
      channel: <T>() => channel as unknown as { onmessage: (event: T) => void },
    });
    const received: string[] = [];
    const subscription = await client.subscribeAgentRun('run/1', {
      afterSequence: '5',
      onEvent: (event) => received.push(event.sequence),
    });

    channel.onmessage({
      type: 'event',
      event: {
        runId: 'run/1',
        sequence: '6',
        occurredAtMs: '1740000000012',
        event: {
          type: 'permission_requested',
          permission: {
            permissionId: 'permission-1',
            runId: 'run/1',
            toolCallId: 'tool-1',
            toolName: 'execute_sql',
            argumentsSha256: 'b'.repeat(64),
            summary: 'Update one order',
            requestedAtMs: '1740000000012',
            expiresAtMs: '1740000060012',
          },
        },
      },
    });
    channel.onmessage({
      type: 'event',
      event: {
        runId: 'run/1',
        sequence: '7',
        occurredAtMs: '1740000000013',
        event: { type: 'completed', messageId: 'message-2' },
      },
    });

    subscription.close();
    channel.onmessage({ type: 'end' });

    expect(received).toEqual(['6', '7']);
    expect(calls[0]).toMatchObject({
      command: 'subscribe_agent_run',
      args: { runId: 'run/1', afterSequence: '5', onEvent: channel },
    });
    await vi.waitFor(() => expect(calls.map(({ command }) => command))
      .toContain('unsubscribe_agent_run'));
    expect(calls.map(({ command }) => command)).toEqual([
      'subscribe_agent_run',
      'unsubscribe_agent_run',
    ]);
    expect(calls.map(({ command }) => command)).not.toContain('cancel_agent_run');
  });

  it('keeps Channel delivery ordered and close remains local', async () => {
    const commands: string[] = [];
    const invoke = async <T>(command: string): Promise<T> => {
      commands.push(command);
      if (command === 'subscribe_operation') {
        return { subscriptionId: 'subscription-1' } as T;
      }
      return undefined as T;
    };
    const channel: { onmessage: (event: OperationStreamMessage) => void } = {
      onmessage: () => undefined,
    };
    const client = new TauriBackendClient({
      invoke,
      channel: <T>() => channel as unknown as { onmessage: (event: T) => void },
    });
    const received: string[] = [];
    const subscription = await client.subscribeOperation('operation-1', {
      afterSequence: '5',
      onEvent: (event) => received.push(event.sequence),
    });
    channel.onmessage({
      type: 'event',
      event: {
        operationId: 'operation-1',
        sequence: '6',
        occurredAtMs: '1740000000000',
        event: { type: 'started' },
      },
    });
    channel.onmessage({
      type: 'event',
      event: {
        operationId: 'operation-1',
        sequence: '7',
        occurredAtMs: '1740000000001',
        event: { type: 'progress', rowCount: '1', byteCount: '8' },
      },
    });

    subscription.close();
    channel.onmessage({
      type: 'event',
      event: {
        operationId: 'operation-1',
        sequence: '8',
        occurredAtMs: '1740000000002',
        event: { type: 'cancelled' },
      },
    });

    expect(received).toEqual(['6', '7']);
    await vi.waitFor(() => expect(commands).toContain('unsubscribe_operation'));
    expect(commands).toEqual(['subscribe_operation', 'unsubscribe_operation']);
    expect(commands).not.toContain('cancel_operation');
  });

  it('normalizes a rejected invoke into ApiRequestError', async () => {
    const client = new TauriBackendClient({
      invoke: async () => Promise.reject({
        code: 'datasource_not_found',
        message: 'Datasource does not exist',
        retryable: false,
      }),
    });

    const error = await client.getDatasource('missing').catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiRequestError);
    expect(error).toMatchObject({
      apiError: { code: 'datasource_not_found', message: 'Datasource does not exist' },
    });
  });

  it('rejects non-monotonic Channel events', async () => {
    const channel: { onmessage: (event: OperationStreamMessage) => void } = {
      onmessage: () => undefined,
    };
    const client = new TauriBackendClient({
      invoke: async <T>(command: string) => (
        command === 'subscribe_operation'
          ? { subscriptionId: 'subscription-1' } as T
          : undefined as T
      ),
      channel: <T>() => channel as unknown as { onmessage: (event: T) => void },
    });
    const onError = vi.fn();
    const onEvent = vi.fn();
    await client.subscribeOperation('operation-1', { afterSequence: '4', onEvent, onError });

    channel.onmessage({
      type: 'event',
      event: {
        operationId: 'operation-1',
        sequence: '4',
        occurredAtMs: '1740000000000',
        event: { type: 'started' },
      },
    });

    expect(onEvent).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(expect.objectContaining({
      apiError: expect.objectContaining({ code: 'invalid_transport_response' }),
    }));
  });

  it('delivers structured Channel errors and clean end notifications', async () => {
    const channels: Array<{ onmessage: (event: OperationStreamMessage) => void }> = [];
    const commands: string[] = [];
    const client = new TauriBackendClient({
      invoke: async <T>(command: string) => {
        commands.push(command);
        return command === 'subscribe_operation'
          ? { subscriptionId: `subscription-${channels.length}` } as T
          : undefined as T;
      },
      channel: <T>() => {
        const channel = { onmessage: (_event: OperationStreamMessage) => undefined };
        channels.push(channel);
        return channel as unknown as { onmessage: (event: T) => void };
      },
    });
    const onError = vi.fn();
    const firstClosed = vi.fn();
    const secondClosed = vi.fn();

    await client.subscribeOperation('operation-1', {
      onEvent: () => undefined,
      onError,
      onClose: firstClosed,
    });
    channels[0]?.onmessage({
      type: 'error',
      error: { code: 'operation_replay_window_expired', message: 'Replay expired', retryable: false },
    });
    await client.subscribeOperation('operation-1', {
      onEvent: () => undefined,
      onClose: secondClosed,
    });
    channels[1]?.onmessage({ type: 'end' });

    expect(onError).toHaveBeenCalledWith(expect.objectContaining({
      apiError: expect.objectContaining({ code: 'operation_replay_window_expired' }),
    }));
    expect(firstClosed).toHaveBeenCalledTimes(1);
    expect(secondClosed).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => {
      expect(commands.filter((command) => command === 'unsubscribe_operation')).toHaveLength(2);
    });
    expect(commands).not.toContain('cancel_operation');
  });

  it.each(['invalid', '-1', '18446744073709551616'])(
    'normalizes malformed replay cursor %s before invoking Rust',
    async (afterSequence) => {
      let invokeCount = 0;
      const invoke = async <T>(): Promise<T> => {
        invokeCount += 1;
        return undefined as T;
      };
      const client = new TauriBackendClient({ invoke });

      await expect(client.subscribeOperation('operation-1', {
        afterSequence,
        onEvent: () => undefined,
      })).rejects.toMatchObject({
        apiError: { code: 'invalid_last_event_id' },
      });
      expect(invokeCount).toBe(0);
    },
  );
});
