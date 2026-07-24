import { describe, expect, it, vi } from 'vitest';

import type { OperationStreamMessage } from './client';
import { ApiRequestError } from './client';
import { TauriBackendClient } from './tauri';

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
