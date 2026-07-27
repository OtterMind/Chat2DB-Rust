// @vitest-environment happy-dom

import { act } from 'react';
import { createRoot, Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  BackendClient,
  CommunityPluginCatalog,
  CommunityTableList,
  Datasource,
  HealthResponse,
} from './backend';
import { CommunityExplorer } from './CommunityExplorer';

type Table = CommunityTableList['items'][number];

const datasource = {
  id: 'source-1',
  name: 'Inventory',
  driverId: 'sha256:mysql',
  hasSecret: true,
  revision: '1',
  createdAtMs: '1',
  updatedAtMs: '1',
} satisfies Datasource;

const compatibility = {
  id: 'community-compatibility',
  label: 'Community compatibility',
  state: 'ready',
  detail: 'Ready',
} satisfies HealthResponse['components'][number];

const catalog = {
  sourceCommit: 'test-commit',
  plugins: [{
    databaseType: 'MYSQL',
    name: 'MySQL',
    behavior: {
      supportsDatabase: true,
      supportsSchema: false,
      preservesScriptBatchExecution: false,
    },
    drivers: [],
    services: {
      metadataAvailable: true,
      sqlBuilderAvailable: true,
      sqlParserAvailable: true,
      dmlBuilderAvailable: true,
      dqlBuilderAvailable: true,
      valueProcessorAvailable: true,
      identifierProcessorAvailable: true,
    },
  }],
} satisfies CommunityPluginCatalog;

function table(name: string): Table {
  return {
    databaseName: 'inventory',
    schemaName: '',
    name,
    tableType: 'BASE TABLE',
    comment: '',
    databaseType: 'MYSQL',
    pinned: false,
    ddl: '',
    engine: 'InnoDB',
    charset: 'utf8mb4',
    collation: 'utf8mb4_0900_ai_ci',
    incrementValue: '0',
    partition: '',
    tablespace: '',
    rows: '0',
    dataLength: '0',
    createTime: '',
    updateTime: '',
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

function fixtureClient() {
  const scope = vi.fn(async () => ({ items: [] }));
  const listCommunitySchemas = vi.fn(async () => ({ items: [] }));
  const listCommunityTables = vi.fn(async () => ({ items: [table('users'), table('orders')] }));
  const client = {
    transport: 'http',
    listCommunityPlugins: vi.fn(async () => catalog),
    listDrivers: vi.fn(async () => ({ items: [] })),
    listCommunityDatabases: vi.fn(async () => ({
      items: [
        { name: 'inventory', comment: '', charset: '', collation: '', owner: '', system: false },
        { name: 'archive', comment: '', charset: '', collation: '', owner: '', system: false },
      ],
    })),
    listCommunitySchemas,
    listCommunityTables,
    listCommunityViews: scope,
    listCommunityFunctions: scope,
    listCommunityProcedures: scope,
    listCommunityTriggers: scope,
    listCommunityColumns: scope,
    listCommunityIndexes: scope,
    listCommunityImportedKeys: scope,
    listCommunityExportedKeys: scope,
    listCommunityPrimaryKeys: scope,
  } as unknown as BackendClient;
  return { client, listCommunitySchemas, listCommunityTables };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true;
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
  throw new Error('Timed out waiting for the table preview UI state');
}

async function click(element: Element): Promise<void> {
  await act(async () => {
    element.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
  });
}

async function setSelectValue(select: HTMLSelectElement, value: string): Promise<void> {
  const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')?.set;
  await act(async () => {
    setter?.call(select, value);
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
}

describe('Community MySQL table preview', () => {
  it('loads a database-only namespace and aborts a repeated preview when its scope changes', async () => {
    const preview = deferred<void>();
    let previewSignal: AbortSignal | undefined;
    const onPreviewTable = vi.fn((_request, signal: AbortSignal) => {
      previewSignal = signal;
      return preview.promise;
    });
    const onCompletionContextChange = vi.fn();
    const fixture = fixtureClient();

    await act(async () => {
      root.render(
        <CommunityExplorer
          client={fixture.client}
          datasource={datasource}
          compatibility={compatibility}
          databaseType="MYSQL"
          onDatabaseTypeChange={vi.fn()}
          onParserAvailabilityChange={vi.fn()}
          onCompletionContextChange={onCompletionContextChange}
          onInsertSql={vi.fn()}
          onPreviewTable={onPreviewTable}
        />,
      );
    });

    await waitFor(() => fixture.listCommunityTables.mock.calls.length > 0 ? true : null);
    expect(fixture.listCommunitySchemas).not.toHaveBeenCalled();
    expect(fixture.listCommunityTables).toHaveBeenCalledWith({
      datasourceId: 'source-1',
      databaseType: 'MYSQL',
      databaseName: 'inventory',
      schemaName: '',
      tableNamePattern: '',
    }, expect.any(AbortSignal));
    expect(onCompletionContextChange).toHaveBeenCalledWith(expect.objectContaining({
      databaseName: 'inventory',
      schemaName: '',
    }));
    expect([...container.querySelectorAll('.explorer-selectors label')]
      .some((label) => label.textContent?.startsWith('Schema'))).toBe(false);

    const users = await waitFor(() => [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('users')));
    await click(users);
    const previewButton = await waitFor(() => {
      const button = container.querySelector<HTMLButtonElement>('button[aria-label="Preview table rows"]');
      return button && !button.disabled ? button : null;
    });
    await click(previewButton);
    await click(previewButton);

    expect(onPreviewTable).toHaveBeenCalledOnce();
    expect(onPreviewTable).toHaveBeenCalledWith({
      datasourceId: 'source-1',
      databaseType: 'MYSQL',
      databaseName: 'inventory',
      schemaName: '',
      tableName: 'users',
    }, expect.any(AbortSignal));

    const databaseSelect = container.querySelectorAll<HTMLSelectElement>('.explorer-selectors select')[1];
    if (!databaseSelect) throw new Error('Database selector is missing');
    await setSelectValue(databaseSelect, 'archive');
    await waitFor(() => previewSignal?.aborted ? true : null);

    preview.resolve();
    await act(async () => { await preview.promise; });
    expect(previewSignal?.aborted).toBe(true);
  });
});
