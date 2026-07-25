import { describe, expect, it, vi } from 'vitest';

import type {
  AgentEvent,
  AgentEventEnvelope,
  AgentRunSnapshot,
  AgentSubscriptionOptions,
} from './client';
import {
  isAgentEventEnvelope,
  isAgentRunSnapshot,
  isAgentStreamMessage,
} from './client';
import { observeAgentRun } from './agent';

function event(sequence: string, value: AgentEvent): AgentEventEnvelope {
  return {
    runId: 'run-1',
    sequence,
    occurredAtMs: '1740000000000',
    event: value,
  };
}

function snapshot(
  lastSequence: string,
  status: AgentRunSnapshot['status'] = 'running',
): AgentRunSnapshot {
  return {
    runId: 'run-1',
    sessionId: 'session-1',
    status,
    lastSequence,
    startedAtMs: '1740000000000',
    updatedAtMs: '1740000000001',
    modelRounds: '2',
    toolCalls: '1',
    usage: { inputTokens: '100', outputTokens: '20', totalTokens: '120' },
  };
}

function permissionRequest() {
  return {
    permissionId: 'permission-1',
    runId: 'run-1',
    toolCallId: 'tool-1',
    toolName: 'execute_sql',
    argumentsSha256: 'a'.repeat(64),
    summary: 'Update one order',
    requestedAtMs: '1740000000001',
    expiresAtMs: '1740000060001',
  };
}

function completion(): { promise: Promise<void>; resolve: () => void } {
  let resolve: () => void = () => undefined;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

describe('agent contract validators', () => {
  it('accepts every AgentEvent variant', () => {
    const events: AgentEvent[] = [
      { type: 'started' },
      { type: 'text_delta', delta: 'hello' },
      {
        type: 'tool_started',
        toolCallId: 'tool-1',
        name: 'execute_sql',
        argumentsSha256: 'a'.repeat(64),
      },
      {
        type: 'tool_completed',
        toolCallId: 'tool-1',
        name: 'execute_sql',
        output: { type: 'text', content: 'done', truncated: false },
      },
      {
        type: 'tool_failed',
        toolCallId: 'tool-1',
        name: 'execute_sql',
        error: { code: 'tool_failed', message: 'Tool failed', retryable: false },
      },
      { type: 'permission_requested', permission: permissionRequest() },
      { type: 'permission_resolved', permissionId: 'permission-1', status: 'approved' },
      { type: 'context_compacted', strategy: 'deterministic_trim', droppedTurns: '4' },
      { type: 'usage', usage: { inputTokens: '100', outputTokens: '20', totalTokens: '120' } },
      { type: 'completed', messageId: 'message-1' },
      { type: 'failed', error: { code: 'provider_failed', message: 'Provider failed' } },
      { type: 'cancelled', reason: null },
    ];

    for (const [index, value] of events.entries()) {
      expect(isAgentEventEnvelope(event(String(index + 1), value))).toBe(true);
    }
  });

  it('rejects stale result-handle and snapshot shapes', () => {
    expect(isAgentEventEnvelope({
      runId: 'run-1',
      sequence: '1',
      occurredAtMs: '1740000000000',
      event: {
        type: 'tool_completed',
        toolCallId: 'tool-1',
        name: 'execute_sql',
        output: {
          type: 'result',
          handle: {
            handleId: 'result-1',
            rowCount: '1',
            byteCount: '8',
            truncatedByMaxRows: false,
            truncatedByMaxResultBytes: false,
            createdAtMs: '1740000000000',
            expiresAtMs: '1740000060000',
            columns: [],
            sampleRows: [],
            sampleTruncated: false,
          },
        },
      },
    })).toBe(false);
    expect(isAgentRunSnapshot({ ...snapshot('1'), status: 'waiting_permission' })).toBe(false);
  });

  it('accepts explicit stream errors and clean ends', () => {
    expect(isAgentStreamMessage({
      type: 'error',
      error: { code: 'agent_failed', message: 'Agent failed', retryable: true },
    })).toBe(true);
    expect(isAgentStreamMessage({ type: 'end' })).toBe(true);
    expect(isAgentStreamMessage({ type: 'event', event: { runId: 'run-1' } })).toBe(false);
  });
});

describe('observeAgentRun', () => {
  it('recovers through waiting_for_permission and filters replay duplicates', async () => {
    const cursors: Array<string | undefined> = [];
    const scripts = [
      (options: AgentSubscriptionOptions) => {
        options.onEvent(event('1', { type: 'text_delta', delta: 'Working' }));
        options.onClose?.();
      },
      (options: AgentSubscriptionOptions) => {
        options.onEvent(event('2', {
          type: 'permission_requested',
          permission: permissionRequest(),
        }));
        options.onEvent(event('3', {
          type: 'permission_resolved',
          permissionId: 'permission-1',
          status: 'approved',
        }));
        options.onEvent(event('4', { type: 'completed', messageId: 'message-1' }));
      },
    ];
    const subscribeAgentRun = vi.fn(async (
      _runId: string,
      options: AgentSubscriptionOptions,
    ) => {
      cursors.push(options.afterSequence);
      const script = scripts.shift();
      queueMicrotask(() => script?.(options));
      return { close: vi.fn() };
    });
    const waitingSnapshot = {
      ...snapshot('2', 'waiting_for_permission'),
      pendingPermission: permissionRequest(),
    };
    const received: string[] = [];
    const snapshots: AgentRunSnapshot['status'][] = [];
    const done = completion();

    observeAgentRun({
      subscribeAgentRun,
      agentRunSnapshot: vi.fn(async () => waitingSnapshot),
    }, 'run-1', {
      onEvent: (value) => received.push(value.sequence),
      onSnapshot: (value) => snapshots.push(value.status),
      onClose: done.resolve,
    });
    await done.promise;

    expect(cursors).toEqual([undefined, '2']);
    expect(received).toEqual(['1', '3', '4']);
    expect(snapshots).toEqual(['waiting_for_permission']);
    expect(subscribeAgentRun).toHaveBeenCalledTimes(2);
  });

  it('stops at a terminal snapshot without opening another stream', async () => {
    const subscribeAgentRun = vi.fn(async (
      _runId: string,
      options: AgentSubscriptionOptions,
    ) => {
      queueMicrotask(() => options.onClose?.());
      return { close: vi.fn() };
    });
    const terminal = { ...snapshot('7', 'failed'), error: {
      code: 'provider_failed',
      message: 'Provider failed',
      retryable: true,
    } };
    const onSnapshot = vi.fn();
    const done = completion();

    observeAgentRun({
      subscribeAgentRun,
      agentRunSnapshot: vi.fn(async () => terminal),
    }, 'run-1', {
      onEvent: () => undefined,
      onSnapshot,
      onClose: done.resolve,
    });
    await done.promise;

    expect(subscribeAgentRun).toHaveBeenCalledTimes(1);
    expect(onSnapshot).toHaveBeenCalledWith(terminal);
  });

  it('turns repeated clean disconnects into a bounded explicit error', async () => {
    const subscribeAgentRun = vi.fn(async (
      _runId: string,
      options: AgentSubscriptionOptions,
    ) => {
      queueMicrotask(() => options.onClose?.());
      return { close: vi.fn() };
    });
    const onError = vi.fn();
    const done = completion();

    observeAgentRun({
      subscribeAgentRun,
      agentRunSnapshot: vi.fn(async () => snapshot('0')),
    }, 'run-1', {
      maxReconnectAttempts: 2,
      onEvent: () => undefined,
      onSnapshot: () => undefined,
      onError,
      onClose: done.resolve,
    });
    await done.promise;

    expect(subscribeAgentRun).toHaveBeenCalledTimes(3);
    expect(onError).toHaveBeenCalledWith(expect.objectContaining({
      apiError: expect.objectContaining({ code: 'agent_stream_reconnect_exhausted' }),
    }));
  });
});
