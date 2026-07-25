import type {
  AgentEventEnvelope,
  AgentRunSnapshot,
  AgentSubscription,
  BackendClient,
} from './client';
import { ApiRequestError, protocolError } from './client';

const DEFAULT_MAX_RECONNECT_ATTEMPTS = 3;

type AgentObserverClient = Pick<BackendClient, 'agentRunSnapshot' | 'subscribeAgentRun'>;

interface StreamOutcome {
  type: 'terminal' | 'end' | 'error';
  error?: Error;
}

export interface AgentObserverOptions {
  afterSequence?: string;
  signal?: AbortSignal;
  maxReconnectAttempts?: number;
  onEvent: (event: AgentEventEnvelope) => void;
  onSnapshot: (snapshot: AgentRunSnapshot) => void;
  onError?: (error: Error) => void;
  onClose?: () => void;
}

function isTerminalEvent(event: AgentEventEnvelope): boolean {
  return event.event.type === 'completed'
    || event.event.type === 'failed'
    || event.event.type === 'cancelled';
}

function isTerminalSnapshot(snapshot: AgentRunSnapshot): boolean {
  return snapshot.status === 'completed'
    || snapshot.status === 'failed'
    || snapshot.status === 'cancelled';
}

function runtimeSequence(value: string, label: string): bigint {
  if (!/^(0|[1-9]\d*)$/.test(value)) {
    throw protocolError(`The runtime returned an invalid ${label}`);
  }
  return BigInt(value);
}

function reconnectExhausted(): ApiRequestError {
  return new ApiRequestError({
    code: 'agent_stream_reconnect_exhausted',
    message: 'The agent event stream disconnected too many times',
    retryable: true,
  });
}

export function observeAgentRun(
  client: AgentObserverClient,
  runId: string,
  options: AgentObserverOptions,
): AgentSubscription {
  const controller = new AbortController();
  const maxReconnectAttempts = Math.max(
    0,
    Math.floor(options.maxReconnectAttempts ?? DEFAULT_MAX_RECONNECT_ATTEMPTS),
  );
  let stopped = false;
  let closeNotified = false;
  let currentSubscription: AgentSubscription | null = null;
  let cursor: bigint | undefined;

  const releaseCurrentSubscription = () => {
    const subscription: AgentSubscription | null = currentSubscription;
    currentSubscription = null;
    subscription?.close();
  };

  const notifyClose = () => {
    if (closeNotified) return;
    closeNotified = true;
    options.signal?.removeEventListener('abort', close);
    options.onClose?.();
  };
  const close = () => {
    if (stopped) return;
    stopped = true;
    controller.abort();
    releaseCurrentSubscription();
    notifyClose();
  };
  const fail = (error: Error) => {
    if (stopped) return;
    options.onError?.(error);
    close();
  };

  if (options.signal?.aborted) close();
  else options.signal?.addEventListener('abort', close, { once: true });

  const consumeStream = (afterSequence?: string): Promise<StreamOutcome> => new Promise((resolve) => {
    let settled = false;
    const settle = (outcome: StreamOutcome) => {
      if (settled) return;
      settled = true;
      resolve(outcome);
    };

    void client.subscribeAgentRun(runId, {
      afterSequence,
      signal: controller.signal,
      onEvent: (event) => {
        if (settled || stopped) return;
        let sequence: bigint;
        try {
          sequence = runtimeSequence(event.sequence, 'agent event sequence');
        } catch (error) {
          settle({ type: 'error', error: error as Error });
          return;
        }
        if (cursor !== undefined && sequence <= cursor) return;
        cursor = sequence;
        options.onEvent(event);
        if (isTerminalEvent(event)) settle({ type: 'terminal' });
      },
      onError: (error) => settle({ type: 'error', error }),
      onClose: () => settle({ type: 'end' }),
    }).then(
      (subscription) => {
        if (settled || stopped) subscription.close();
        else currentSubscription = subscription;
      },
      (error: unknown) => settle({
        type: 'error',
        error: error instanceof Error ? error : protocolError('Agent stream failed'),
      }),
    );
  });

  void (async () => {
    try {
      if (options.afterSequence !== undefined) {
        cursor = runtimeSequence(options.afterSequence, 'agent replay cursor');
      }
      let reconnectAttempts = 0;
      while (!stopped) {
        const outcome = await consumeStream(cursor?.toString());
        releaseCurrentSubscription();
        if (stopped || outcome.type === 'terminal') return;

        let snapshot: AgentRunSnapshot;
        try {
          snapshot = await client.agentRunSnapshot(runId, controller.signal);
        } catch (error) {
          if (!stopped) {
            fail(error instanceof Error ? error : protocolError('Agent recovery failed'));
          }
          return;
        }
        if (stopped) return;
        if (snapshot.runId !== runId) {
          fail(protocolError('The runtime returned a snapshot for another agent run'));
          return;
        }

        let snapshotSequence: bigint;
        try {
          snapshotSequence = runtimeSequence(snapshot.lastSequence, 'agent snapshot sequence');
        } catch (error) {
          fail(error as Error);
          return;
        }
        if (cursor !== undefined && snapshotSequence < cursor) {
          fail(protocolError('The runtime returned a stale agent snapshot'));
          return;
        }
        cursor = snapshotSequence;
        options.onSnapshot(snapshot);
        if (isTerminalSnapshot(snapshot)) return;

        if (reconnectAttempts >= maxReconnectAttempts) {
          fail(outcome.error ?? reconnectExhausted());
          return;
        }
        reconnectAttempts += 1;
      }
    } catch (error) {
      fail(error instanceof Error ? error : protocolError('Agent observation failed'));
    } finally {
      releaseCurrentSubscription();
      notifyClose();
    }
  })();

  return { close };
}
