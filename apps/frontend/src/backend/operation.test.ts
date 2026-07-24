import { describe, expect, it, vi } from 'vitest';

import type {
  OperationEventEnvelope,
  OperationSnapshot,
  OperationSubscriptionOptions,
} from './client';
import { ApiRequestError } from './client';
import { observeOperation } from './operation';

function event(sequence: string, type: 'started' | 'cancelled' = 'started'): OperationEventEnvelope {
  return {
    operationId: 'operation-1',
    sequence,
    occurredAtMs: '1740000000000',
    event: type === 'started' ? { type } : { type },
  };
}

function runningSnapshot(lastSequence: string): OperationSnapshot {
  return {
    operationId: 'operation-1',
    status: 'running',
    lastSequence,
    startedAtMs: '1740000000000',
    updatedAtMs: '1740000000001',
    rowCount: '10',
    byteCount: '80',
  };
}

function completion(): { promise: Promise<void>; resolve: () => void } {
  let resolve: () => void = () => undefined;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

describe('observeOperation', () => {
  it('recovers a clean end from snapshot lastSequence and filters replay duplicates', async () => {
    const cursors: Array<string | undefined> = [];
    const scripts = [
      (options: OperationSubscriptionOptions) => {
        options.onEvent(event('1'));
        options.onClose?.();
      },
      (options: OperationSubscriptionOptions) => {
        options.onEvent(event('2'));
        options.onEvent(event('3', 'cancelled'));
      },
    ];
    const subscribeOperation = vi.fn(async (
      _operationId: string,
      options: OperationSubscriptionOptions,
    ) => {
      cursors.push(options.afterSequence);
      const script = scripts.shift();
      queueMicrotask(() => script?.(options));
      return { close: vi.fn() };
    });
    const operationSnapshot = vi.fn(async () => runningSnapshot('2'));
    const received: string[] = [];
    const snapshots: string[] = [];
    const done = completion();

    observeOperation({ subscribeOperation, operationSnapshot }, 'operation-1', {
      onEvent: (value) => received.push(value.sequence),
      onSnapshot: (value) => snapshots.push(value.lastSequence),
      onClose: done.resolve,
    });
    await done.promise;

    expect(cursors).toEqual([undefined, '2']);
    expect(received).toEqual(['1', '3']);
    expect(snapshots).toEqual(['2']);
    expect(operationSnapshot).toHaveBeenCalledTimes(1);
  });

  it('recovers a typed stream error and continues from the materialized cursor', async () => {
    const cursors: Array<string | undefined> = [];
    const scripts = [
      (options: OperationSubscriptionOptions) => options.onError?.(new ApiRequestError({
        code: 'operation_replay_window_expired',
        message: 'Replay expired',
        retryable: false,
      })),
      (options: OperationSubscriptionOptions) => options.onEvent(event('5', 'cancelled')),
    ];
    const subscribeOperation = vi.fn(async (
      _operationId: string,
      options: OperationSubscriptionOptions,
    ) => {
      cursors.push(options.afterSequence);
      const script = scripts.shift();
      queueMicrotask(() => script?.(options));
      return { close: vi.fn() };
    });
    const done = completion();
    const onError = vi.fn();

    observeOperation({
      subscribeOperation,
      operationSnapshot: vi.fn(async () => runningSnapshot('4')),
    }, 'operation-1', {
      onEvent: () => undefined,
      onSnapshot: () => undefined,
      onError,
      onClose: done.resolve,
    });
    await done.promise;

    expect(cursors).toEqual([undefined, '4']);
    expect(onError).not.toHaveBeenCalled();
  });

  it('stops at a terminal snapshot without opening another stream', async () => {
    const subscribeOperation = vi.fn(async (
      _operationId: string,
      options: OperationSubscriptionOptions,
    ) => {
      queueMicrotask(() => options.onClose?.());
      return { close: vi.fn() };
    });
    const terminal = { ...runningSnapshot('7'), status: 'cancelled' as const };
    const onSnapshot = vi.fn();
    const done = completion();

    observeOperation({
      subscribeOperation,
      operationSnapshot: vi.fn(async () => terminal),
    }, 'operation-1', {
      onEvent: () => undefined,
      onSnapshot,
      onClose: done.resolve,
    });
    await done.promise;

    expect(subscribeOperation).toHaveBeenCalledTimes(1);
    expect(onSnapshot).toHaveBeenCalledWith(terminal);
  });

  it('turns repeated clean disconnects into a bounded explicit error', async () => {
    const subscribeOperation = vi.fn(async (
      _operationId: string,
      options: OperationSubscriptionOptions,
    ) => {
      queueMicrotask(() => options.onClose?.());
      return { close: vi.fn() };
    });
    const onError = vi.fn();
    const done = completion();

    observeOperation({
      subscribeOperation,
      operationSnapshot: vi.fn(async () => runningSnapshot('0')),
    }, 'operation-1', {
      maxReconnectAttempts: 2,
      onEvent: () => undefined,
      onSnapshot: () => undefined,
      onError,
      onClose: done.resolve,
    });
    await done.promise;

    expect(subscribeOperation).toHaveBeenCalledTimes(3);
    expect(onError).toHaveBeenCalledWith(expect.objectContaining({
      apiError: expect.objectContaining({ code: 'operation_stream_reconnect_exhausted' }),
    }));
  });
});
