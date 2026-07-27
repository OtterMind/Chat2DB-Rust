// @vitest-environment happy-dom

import { act } from 'react';
import { createRoot, Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  BackendClient,
  HealthResponse,
  OperationSubscriptionOptions,
  ResultPage,
} from './backend';
import App from './App';

const previewHarness = vi.hoisted(() => ({
  controller: undefined as AbortController | undefined,
}));

vi.mock('./CommunityExplorer', () => ({
  CommunityExplorer: ({
    onPreviewTable,
    previewDisabled,
  }: {
    onPreviewTable: (request: {
      datasourceId: string;
      databaseType: string;
      databaseName: string;
      schemaName: string;
      tableName: string;
    }, signal: AbortSignal) => Promise<void>;
    previewDisabled?: boolean;
  }) => (
    <button
      type="button"
      aria-label="Test table preview"
      disabled={previewDisabled}
      onClick={() => {
        previewHarness.controller = new AbortController();
        void onPreviewTable({
          datasourceId: 'source-1',
          databaseType: 'MYSQL',
          databaseName: 'inventory',
          schemaName: '',
          tableName: 'users',
        }, previewHarness.controller.signal);
      }}
    >Preview</button>
  ),
}));

const health = {
  components: [],
  product: { edition: 'community', name: 'Chat2DB', version: '0.1.0' },
  status: 'ready',
  uptimeSeconds: 1,
} satisfies HealthResponse;

const datasource = {
  id: 'source-1',
  name: 'Inventory',
  driverId: 'sha256:mysql',
  hasSecret: true,
  revision: '1',
  createdAtMs: '1',
  updatedAtMs: '1',
};

const metadata = {
  id: 'result-preview-1',
  rowCount: '1',
  byteCount: '5',
  createdAtMs: '1740000000000',
  expiresAtMs: '1740003600000',
  truncatedByMaxRows: false,
  truncatedByMaxResultBytes: false,
};

const page = {
  metadata,
  offset: '0',
  hasMore: false,
  columns: [{
    ordinal: 1,
    label: 'name',
    name: 'name',
    jdbcType: 12,
    jdbcTypeName: 'VARCHAR',
    nullability: 'nullable',
    valueType: 'text',
  }],
  rows: [{ values: [{ type: 'text', value: 'Alice' }] }],
} satisfies ResultPage;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

function fixtureClient(startCommunityTablePreview: BackendClient['startCommunityTablePreview']) {
  const cancelOperation = vi.fn(async () => ({ disposition: 'accepted' as const }));
  const resultPage = vi.fn(async () => page);
  const subscribeOperation = vi.fn(async (
    operationId: string,
    options: OperationSubscriptionOptions,
  ) => {
    queueMicrotask(() => {
      options.onEvent({
        operationId,
        sequence: '1',
        occurredAtMs: '1740000000001',
        event: { type: 'completed', result: metadata },
      });
      options.onClose?.();
    });
    return { close: vi.fn() };
  });
  const client = {
    transport: 'http',
    health: vi.fn(async () => health),
    listDrivers: vi.fn(async () => ({ items: [] })),
    listDatasources: vi.fn(async () => ({ items: [datasource] })),
    startCommunityTablePreview,
    subscribeOperation,
    operationSnapshot: vi.fn(),
    resultPage,
    cancelOperation,
  } as unknown as BackendClient;
  return { client, cancelOperation, resultPage, subscribeOperation };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true;
  previewHarness.controller = undefined;
  container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  document.body.replaceChildren();
});

async function waitFor<T>(read: () => T | null | undefined): Promise<T> {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    const value = read();
    if (value !== null && value !== undefined) return value;
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  }
  throw new Error('Timed out waiting for the App table preview state');
}

describe('App Community table preview operation', () => {
  it('writes generated SQL and renders the observed result page', async () => {
    const sql = 'SELECT * FROM `inventory`.`users` LIMIT 200';
    const startCommunityTablePreview = vi.fn(async () => ({
      operationId: 'operation-preview-1',
      sql,
      rowLimit: 200,
    }));
    const fixture = fixtureClient(startCommunityTablePreview);
    await act(async () => root.render(<App client={fixture.client} />));

    const previewButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Test table preview"]',
    );
    if (!previewButton) throw new Error('Preview harness button is missing');
    await act(async () => previewButton.click());

    await waitFor(() => container.querySelector<HTMLTextAreaElement>('textarea')?.value === sql ? true : null);
    await waitFor(() => container.textContent?.includes('Alice') ? true : null);

    expect(startCommunityTablePreview).toHaveBeenCalledWith({
      datasourceId: 'source-1',
      databaseType: 'MYSQL',
      databaseName: 'inventory',
      schemaName: '',
      tableName: 'users',
    }, expect.any(AbortSignal));
    expect(fixture.subscribeOperation).toHaveBeenCalledWith(
      'operation-preview-1',
      expect.objectContaining({ afterSequence: undefined }),
    );
    expect(fixture.resultPage).toHaveBeenCalledWith('result-preview-1', {
      offset: '0',
      maxRows: '50',
      maxBytes: '8388608',
    });
  });

  it('cancels an operation accepted after the preview scope was aborted', async () => {
    const response = deferred<{ operationId: string; sql: string; rowLimit: number }>();
    const startCommunityTablePreview = vi.fn(() => response.promise);
    const fixture = fixtureClient(startCommunityTablePreview);
    await act(async () => root.render(<App client={fixture.client} />));

    const previewButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Test table preview"]',
    );
    if (!previewButton) throw new Error('Preview harness button is missing');
    await act(async () => previewButton.click());
    await waitFor(() => startCommunityTablePreview.mock.calls.length > 0 ? true : null);

    await act(async () => previewHarness.controller?.abort());
    response.resolve({
      operationId: 'operation-preview-late',
      sql: 'SELECT * FROM `archive`.`users` LIMIT 200',
      rowLimit: 200,
    });
    await act(async () => { await response.promise; });
    await waitFor(() => fixture.cancelOperation.mock.calls.length > 0 ? true : null);

    expect(container.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe('SELECT 1;');
    expect(fixture.subscribeOperation).not.toHaveBeenCalled();
    expect(fixture.cancelOperation).toHaveBeenCalledWith('operation-preview-late');
  });
});
