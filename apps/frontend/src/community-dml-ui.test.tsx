// @vitest-environment happy-dom

import { act } from 'react';
import { createRoot, Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  BackendClient,
  BuildCommunityDmlRequest,
  CommunityBuiltSql,
  CommunityPluginCatalog,
  CommunityPrimaryKeyList,
  CommunityTableColumnList,
  CommunityTableList,
  Datasource,
  HealthResponse,
} from './backend';
import { CommunityDmlDialog } from './CommunityDmlDialog';
import { CommunityExplorer } from './CommunityExplorer';

type Table = CommunityTableList['items'][number];
type Column = CommunityTableColumnList['items'][number];

const datasource = {
  id: 'source-1',
  name: 'Inventory',
  driverId: 'sha256:h2',
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
    databaseType: 'H2',
    name: 'H2',
    behavior: {
      supportsDatabase: true,
      supportsSchema: true,
      preservesScriptBatchExecution: false,
    },
    drivers: [],
    services: {
      metadataAvailable: true,
      sqlBuilderAvailable: true,
      sqlParserAvailable: true,
      dmlBuilderAvailable: true,
      valueProcessorAvailable: true,
      identifierProcessorAvailable: true,
    },
  }],
} satisfies CommunityPluginCatalog;

function table(name: string): Table {
  return {
    databaseName: 'inventory',
    schemaName: 'APP',
    name,
    tableType: 'BASE TABLE',
    comment: '',
    databaseType: 'H2',
    pinned: false,
    ddl: '',
    engine: '',
    charset: '',
    collation: '',
    incrementValue: '0',
    partition: '',
    tablespace: '',
    rows: '0',
    dataLength: '0',
    createTime: '',
    updateTime: '',
  };
}

function column(tableName: string, name: string, columnType: string, patch: Partial<Column> = {}): Column {
  return {
    databaseName: 'inventory',
    schemaName: 'APP',
    tableName,
    name,
    columnType,
    defaultValue: '',
    comment: '',
    primaryKeyName: '',
    primaryKeyOrder: 0,
    extent: '',
    charset: '',
    collation: '',
    unit: '',
    defaultConstraintName: '',
    ...patch,
  };
}

const itemsColumns = [
  column('ITEMS', 'ID', 'BIGINT', {
    primaryKey: true,
    primaryKeyName: 'PK_ITEMS',
    primaryKeyOrder: 1,
    columnSize: 64,
    decimalDigits: 0,
  }),
  column('ITEMS', 'LABEL', 'VARCHAR', { columnSize: 255 }),
];

const primaryKeys = [{
  databaseName: 'inventory',
  schemaName: 'APP',
  tableName: 'ITEMS',
  columnName: 'ID',
  name: 'PK_ITEMS',
}] satisfies CommunityPrimaryKeyList['items'];

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

function fixtureClient(buildCommunityDml: BackendClient['buildCommunityDml']) {
  const startQuery = vi.fn();
  const client = {
    transport: 'http',
    listCommunityPlugins: vi.fn(async () => catalog),
    listDrivers: vi.fn(async () => ({ items: [] })),
    listCommunityDatabases: vi.fn(async () => ({
      items: [{
        name: 'inventory',
        comment: '',
        charset: '',
        collation: '',
        owner: '',
        system: false,
      }],
    })),
    listCommunitySchemas: vi.fn(async () => ({
      items: [{
        databaseName: 'inventory',
        name: 'APP',
        comment: '',
        owner: '',
        system: false,
      }],
    })),
    listCommunityTables: vi.fn(async () => ({ items: [table('ITEMS'), table('ORDERS')] })),
    listCommunityViews: vi.fn(async () => ({ items: [] })),
    listCommunityFunctions: vi.fn(async () => ({ items: [] })),
    listCommunityProcedures: vi.fn(async () => ({ items: [] })),
    listCommunityTriggers: vi.fn(async () => ({ items: [] })),
    listCommunityColumns: vi.fn(async (request: { tableName: string }) => ({
      items: request.tableName === 'ITEMS'
        ? itemsColumns
        : [column('ORDERS', 'ID', 'BIGINT', { primaryKey: true })],
    })),
    listCommunityIndexes: vi.fn(async () => ({ items: [] })),
    listCommunityImportedKeys: vi.fn(async () => ({ items: [] })),
    listCommunityExportedKeys: vi.fn(async () => ({ items: [] })),
    listCommunityPrimaryKeys: vi.fn(async (request: { tableName: string }) => ({
      items: request.tableName === 'ITEMS' ? primaryKeys : [],
    })),
    buildCommunityDml,
    startQuery,
  } as unknown as BackendClient;
  return { client, startQuery };
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
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const value = read();
    if (value !== null && value !== undefined) return value;
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  }
  throw new Error('Timed out waiting for the UI state');
}

async function click(element: Element): Promise<void> {
  await act(async () => {
    element.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
  });
}

async function setInputValue(input: HTMLInputElement, value: string): Promise<void> {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
  await act(async () => {
    setter?.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
}

function buttonWithText(text: string): HTMLButtonElement | undefined {
  return [...container.querySelectorAll<HTMLButtonElement>('button')]
    .find((button) => button.textContent?.includes(text));
}

async function renderExplorer(buildCommunityDml: BackendClient['buildCommunityDml']) {
  const callbacks = {
    onDatabaseTypeChange: vi.fn(),
    onParserAvailabilityChange: vi.fn(),
    onCompletionContextChange: vi.fn(),
    onInsertSql: vi.fn(),
  };
  const fixture = fixtureClient(buildCommunityDml);
  await act(async () => {
    root.render(
      <CommunityExplorer
        client={fixture.client}
        datasource={datasource}
        compatibility={compatibility}
        databaseType="H2"
        {...callbacks}
      />,
    );
  });
  const itemsButton = await waitFor(() => buttonWithText('ITEMS'));
  await click(itemsButton);
  const buildButton = await waitFor(() => {
    const button = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Build INSERT or UPDATE SQL"]',
    );
    return button && !button.disabled ? button : null;
  });
  await click(buildButton);
  await waitFor(() => container.querySelector<HTMLElement>('[role="dialog"]'));
  return { ...fixture, ...callbacks };
}

async function submitInsert(): Promise<void> {
  const idInput = await waitFor(() => container.querySelector<HTMLInputElement>(
    'input[aria-label="INSERT ID value"]',
  ));
  const labelInput = await waitFor(() => container.querySelector<HTMLInputElement>(
    'input[aria-label="INSERT LABEL value"]',
  ));
  await setInputValue(idInput, '7');
  await setInputValue(labelInput, "O'Brien");
  const form = container.querySelector<HTMLFormElement>('[role="dialog"] form');
  if (!form) throw new Error('DML form is missing');
  await act(async () => {
    form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await Promise.resolve();
  });
}

describe('Community DML dialog interaction', () => {
  it('traps keyboard focus, closes from Escape or backdrop, and restores focus', async () => {
    const returnTarget = document.createElement('button');
    document.body.prepend(returnTarget);
    returnTarget.focus();
    const onClose = vi.fn();
    await act(async () => {
      root.render(
        <CommunityDmlDialog
          databaseType="H2"
          databaseName="inventory"
          schemaName="APP"
          tableName="ITEMS"
          columns={itemsColumns}
          primaryKeys={primaryKeys}
          busy={false}
          error={null}
          onClose={onClose}
          onBuild={vi.fn(async () => undefined)}
        />,
      );
    });

    const dialog = container.querySelector<HTMLElement>('[role="dialog"]');
    const backdrop = container.querySelector<HTMLElement>('.dialog-backdrop');
    const closeButton = container.querySelector<HTMLButtonElement>('button[aria-label="Close"]');
    const submitButton = buttonWithText('Use SQL');
    expect(dialog).not.toBeNull();
    expect(backdrop).not.toBeNull();
    expect(dialog?.contains(document.activeElement)).toBe(true);

    submitButton?.focus();
    dialog?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    expect(document.activeElement).toBe(closeButton);
    closeButton?.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'Tab',
      shiftKey: true,
      bubbles: true,
    }));
    expect(document.activeElement).toBe(submitButton);

    dialog?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(onClose).not.toHaveBeenCalled();
    backdrop?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(onClose).toHaveBeenCalledTimes(1);
    dialog?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onClose).toHaveBeenCalledTimes(2);

    await act(async () => root.render(null));
    expect(document.activeElement).toBe(returnTarget);
  });
});

describe('Community DML explorer lifecycle', () => {
  it('writes generated SQL to the editor boundary without starting a query', async () => {
    const builtSql = "INSERT INTO inventory.APP.ITEMS (ID, LABEL) VALUES (7, 'O''Brien')";
    const buildCommunityDml = vi.fn(async (): Promise<CommunityBuiltSql> => ({ sql: builtSql }));
    const fixture = await renderExplorer(buildCommunityDml);

    await submitInsert();
    await waitFor(() => fixture.onInsertSql.mock.calls.length > 0 ? true : null);

    expect(buildCommunityDml).toHaveBeenCalledOnce();
    expect(fixture.onInsertSql).toHaveBeenCalledWith(builtSql);
    expect(fixture.startQuery).not.toHaveBeenCalled();
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });

  it('aborts and rejects a late response after refresh closes the dialog', async () => {
    const response = deferred<CommunityBuiltSql>();
    let signal: AbortSignal | undefined;
    const buildCommunityDml = vi.fn((
      _request: BuildCommunityDmlRequest,
      requestSignal?: AbortSignal,
    ) => {
      signal = requestSignal;
      return response.promise;
    });
    const fixture = await renderExplorer(buildCommunityDml);
    await submitInsert();

    const refresh = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Retry and refresh objects"]',
    );
    if (!refresh) throw new Error('Refresh button is missing');
    await click(refresh);
    await waitFor(() => container.querySelector('[role="dialog"]') === null ? true : null);

    expect(signal?.aborted).toBe(true);
    response.resolve({ sql: 'LATE REFRESH SQL' });
    await act(async () => { await Promise.resolve(); });
    expect(fixture.onInsertSql).not.toHaveBeenCalled();
    expect(fixture.startQuery).not.toHaveBeenCalled();
  });

  it('aborts and rejects a late response after the selected table changes', async () => {
    const response = deferred<CommunityBuiltSql>();
    let signal: AbortSignal | undefined;
    const buildCommunityDml = vi.fn((
      _request: BuildCommunityDmlRequest,
      requestSignal?: AbortSignal,
    ) => {
      signal = requestSignal;
      return response.promise;
    });
    const fixture = await renderExplorer(buildCommunityDml);
    await submitInsert();

    const ordersButton = buttonWithText('ORDERS');
    if (!ordersButton) throw new Error('ORDERS button is missing');
    await click(ordersButton);
    await waitFor(() => container.querySelector('[role="dialog"]') === null ? true : null);

    expect(signal?.aborted).toBe(true);
    response.resolve({ sql: 'LATE TABLE SQL' });
    await act(async () => { await Promise.resolve(); });
    expect(fixture.onInsertSql).not.toHaveBeenCalled();
    expect(fixture.startQuery).not.toHaveBeenCalled();
  });
});
