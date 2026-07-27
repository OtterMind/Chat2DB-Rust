// @vitest-environment happy-dom

import { act } from 'react';
import { createRoot, Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  BackendClient,
  BuildCommunityNamespaceSqlRequest,
  CommunityBuiltSql,
  CommunityDatabase,
  CommunityPluginCatalog,
  Datasource,
  HealthResponse,
} from './backend';
import { CommunityExplorer } from './CommunityExplorer';
import { CommunityNamespaceDialog } from './CommunityNamespaceDialog';

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

const inventory = {
  name: 'inventory',
  comment: '',
  charset: '',
  collation: '',
  owner: '',
  system: false,
} satisfies CommunityDatabase;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

function fixtureClient(
  buildCommunityNamespaceSql: BackendClient['buildCommunityNamespaceSql'],
) {
  const startQuery = vi.fn();
  const client = {
    transport: 'http',
    listCommunityPlugins: vi.fn(async () => catalog),
    listDrivers: vi.fn(async () => ({ items: [] })),
    listCommunityDatabases: vi.fn(async () => ({
      items: [inventory, { ...inventory, name: 'archive' }],
    })),
    listCommunitySchemas: vi.fn(async (request: { databaseName: string }) => ({
      items: [{
        databaseName: request.databaseName,
        name: request.databaseName === 'inventory' ? 'APP' : 'PUBLIC',
        comment: '',
        owner: '',
        system: false,
      }],
    })),
    listCommunityTables: vi.fn(async () => ({ items: [] })),
    listCommunityViews: vi.fn(async () => ({ items: [] })),
    listCommunityFunctions: vi.fn(async () => ({ items: [] })),
    listCommunityProcedures: vi.fn(async () => ({ items: [] })),
    listCommunityTriggers: vi.fn(async () => ({ items: [] })),
    buildCommunityNamespaceSql,
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

async function setSelectValue(select: HTMLSelectElement, value: string): Promise<void> {
  const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')?.set;
  await act(async () => {
    setter?.call(select, value);
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
}

async function renderExplorer(
  buildCommunityNamespaceSql: BackendClient['buildCommunityNamespaceSql'],
) {
  const callbacks = {
    onDatabaseTypeChange: vi.fn(),
    onParserAvailabilityChange: vi.fn(),
    onCompletionContextChange: vi.fn(),
    onInsertSql: vi.fn(),
  };
  const fixture = fixtureClient(buildCommunityNamespaceSql);
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
  const buildButton = await waitFor(() => {
    const button = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Build database or schema SQL"]',
    );
    return button && !button.disabled ? button : null;
  });
  await click(buildButton);
  await waitFor(() => container.querySelector<HTMLElement>('[role="dialog"]'));
  return { ...fixture, ...callbacks };
}

async function submitCreateSchema(schemaName: string): Promise<void> {
  const input = await waitFor(() => container.querySelector<HTMLInputElement>(
    'input[aria-label="Schema name"]',
  ));
  await setInputValue(input, schemaName);
  const form = container.querySelector<HTMLFormElement>('[role="dialog"] form');
  if (!form) throw new Error('Namespace form is missing');
  await act(async () => {
    form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await Promise.resolve();
  });
}

describe('Community namespace dialog', () => {
  it('offers all seven closed operations without a raw SQL input', async () => {
    await act(async () => {
      root.render(
        <CommunityNamespaceDialog
          databaseType="H2"
          database={inventory}
          schemaName="APP"
          supportsDatabase
          supportsSchema
          busy={false}
          error={null}
          onClose={vi.fn()}
          onBuild={vi.fn(async () => undefined)}
        />,
      );
    });

    const operation = container.querySelector<HTMLSelectElement>(
      'select[aria-label="Namespace operation"]',
    );
    expect([...operation!.options].map((option) => option.value)).toEqual([
      'createDatabase',
      'alterDatabase',
      'dropDatabase',
      'useDatabase',
      'createSchema',
      'alterSchema',
      'dropSchema',
    ]);
    expect(container.querySelector('textarea')).toBeNull();
    expect(container.querySelector('input[aria-label="SQL"]')).toBeNull();
  });
});

describe('Community namespace explorer lifecycle', () => {
  it('writes generated SQL to the editor boundary without starting a query', async () => {
    const builtSql = 'CREATE SCHEMA reporting';
    const buildCommunityNamespaceSql = vi.fn(async (): Promise<CommunityBuiltSql> => ({ sql: builtSql }));
    const fixture = await renderExplorer(buildCommunityNamespaceSql);

    await submitCreateSchema('reporting');
    await waitFor(() => fixture.onInsertSql.mock.calls.length > 0 ? true : null);

    expect(buildCommunityNamespaceSql).toHaveBeenCalledWith({
      databaseType: 'H2',
      operation: {
        kind: 'createSchema',
        schema: {
          databaseName: 'inventory',
          name: 'reporting',
          comment: '',
          owner: '',
          system: false,
        },
      },
    }, expect.any(AbortSignal));
    expect(fixture.onInsertSql).toHaveBeenCalledWith(builtSql);
    expect(fixture.startQuery).not.toHaveBeenCalled();
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });

  it('aborts and rejects a late response when the namespace scope changes', async () => {
    const response = deferred<CommunityBuiltSql>();
    let signal: AbortSignal | undefined;
    const buildCommunityNamespaceSql = vi.fn((
      _request: BuildCommunityNamespaceSqlRequest,
      requestSignal?: AbortSignal,
    ) => {
      signal = requestSignal;
      return response.promise;
    });
    const fixture = await renderExplorer(buildCommunityNamespaceSql);
    await submitCreateSchema('reporting');

    const selectors = container.querySelectorAll<HTMLSelectElement>('.explorer-selectors select');
    const databaseSelect = selectors[1];
    if (!databaseSelect) throw new Error('Database selector is missing');
    await setSelectValue(databaseSelect, 'archive');
    await waitFor(() => container.querySelector('[role="dialog"]') === null ? true : null);

    expect(signal?.aborted).toBe(true);
    response.resolve({ sql: 'LATE NAMESPACE SQL' });
    await act(async () => { await Promise.resolve(); });
    expect(fixture.onInsertSql).not.toHaveBeenCalled();
    expect(fixture.startQuery).not.toHaveBeenCalled();
  });
});
