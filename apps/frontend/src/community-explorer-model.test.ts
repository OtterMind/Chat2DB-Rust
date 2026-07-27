import { describe, expect, it, vi } from 'vitest';

import type {
  CommunityForeignKeyList,
  CommunityFunction,
  CommunityFunctionList,
  CommunityFunctionParameterList,
  CommunityPluginCatalog,
  CommunityPrimaryKeyList,
  CommunityProcedure,
  CommunityProcedureParameterList,
  CommunityTableColumnList,
  CommunityTableIndexList,
  CommunityTableList,
  CommunityTrigger,
  CommunityViewList,
  JdbcDriverList,
} from './backend/client';
import {
  type CommunityFunctionDetailClient,
  type CommunityNamespaceClient,
  type CommunityNamespaceSnapshot,
  type CommunityProcedureDetailClient,
  type CommunityTableDetail,
  type CommunityTableDetailClient,
  type CommunityTriggerDetailClient,
  loadCommunityFunctionDetail,
  loadCommunityNamespace,
  loadCommunityProcedureDetail,
  loadCommunityTableDetail,
  loadCommunityTriggerDetail,
  selectCommunityItem,
  selectCommunityPlugin,
} from './community-explorer-model';

const namespaceRequest = {
  datasourceId: 'datasource-1',
  databaseType: 'H2',
  databaseName: 'inventory',
  schemaName: 'APP',
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => { resolve = promiseResolve; });
  return { promise, resolve };
}

const table = {
  databaseName: 'inventory',
  schemaName: 'APP',
  name: 'ITEMS',
  tableType: 'BASE TABLE',
  comment: '',
  databaseType: 'H2',
  pinned: false,
  ddl: '',
  engine: '',
  charset: '',
  collation: '',
  partition: '',
  tablespace: '',
  createTime: '',
  updateTime: '',
} satisfies CommunityTableList['items'][number];

const view = {
  ...table,
  name: 'ITEMS_VIEW',
  tableType: 'VIEW',
} satisfies CommunityViewList['items'][number];

const functionMetadata = {
  databaseName: 'inventory',
  schemaName: 'APP',
  name: 'DOUBLE_VALUE',
  remarks: '',
  functionType: 1,
  specificName: 'DOUBLE_VALUE_1',
  body: 'return value * 2;',
  template: '',
} satisfies CommunityFunction;

const procedureMetadata = {
  databaseName: 'inventory',
  schemaName: 'APP',
  name: 'REFRESH_ITEMS',
  remarks: '',
  procedureType: 1,
  specificName: 'REFRESH_ITEMS_1',
  body: 'call refresh_items();',
} satisfies CommunityProcedure;

const triggerMetadata = {
  databaseName: 'inventory',
  schemaName: 'APP',
  name: 'ITEMS_AUDIT',
  eventManipulation: 'INSERT',
  body: 'audit.ItemsTrigger',
} satisfies CommunityTrigger;

const column = {
  databaseName: 'inventory',
  schemaName: 'APP',
  tableName: 'ITEMS',
  name: 'ID',
  columnType: 'BIGINT',
  defaultValue: '',
  comment: '',
  primaryKeyName: 'PK_ITEMS',
  primaryKeyOrder: 1,
  extent: '',
  charset: '',
  collation: '',
  unit: '',
  defaultConstraintName: '',
} satisfies CommunityTableColumnList['items'][number];

const index = {
  databaseName: 'inventory',
  schemaName: 'APP',
  tableName: 'ITEMS',
  name: 'IDX_ITEMS',
  indexType: 'BTREE',
  comment: '',
  columns: [],
  method: '',
  foreignSchemaName: '',
  foreignTableName: '',
  foreignColumnNames: [],
} satisfies CommunityTableIndexList['items'][number];

const importedKey = {
  primaryTableDatabase: 'inventory',
  primaryTableSchema: 'APP',
  primaryTableName: 'PARENTS',
  primaryColumnName: 'ID',
  foreignTableDatabase: 'inventory',
  foreignTableSchema: 'APP',
  foreignTableName: 'ITEMS',
  foreignColumnName: 'PARENT_ID',
  keySequence: 1,
  updateRule: 1,
  deleteRule: 1,
  foreignKeyName: 'FK_ITEMS_PARENT',
  primaryKeyName: 'PK_PARENTS',
  deferrability: 7,
} satisfies CommunityForeignKeyList['items'][number];

const exportedKey = {
  ...importedKey,
  primaryTableName: 'ITEMS',
  foreignTableName: 'AUDIT_ITEMS',
  foreignKeyName: 'FK_AUDIT_ITEMS',
} satisfies CommunityForeignKeyList['items'][number];

const primaryKey = {
  databaseName: 'inventory',
  schemaName: 'APP',
  tableName: 'ITEMS',
  columnName: 'ID',
  name: 'PK_ITEMS',
} satisfies CommunityPrimaryKeyList['items'][number];

const functionParameter = {
  functionDatabase: 'inventory',
  functionSchema: 'APP',
  functionName: 'DOUBLE_VALUE',
  columnName: 'VALUE',
  columnType: 1,
  dataType: 4,
  typeName: 'INTEGER',
  precision: 32,
  length: 4,
  scale: 0,
  radix: 10,
  nullable: 1,
  remarks: '',
  charOctetLength: 4,
  ordinalPosition: 1,
  isNullable: 'YES',
  specificName: 'DOUBLE_VALUE_1',
} satisfies CommunityFunctionParameterList['items'][number];

const procedureParameter = {
  procedureDatabase: 'inventory',
  procedureSchema: 'APP',
  procedureName: 'REFRESH_ITEMS',
  columnName: 'LIMIT_VALUE',
  columnType: 1,
  dataType: 4,
  typeName: 'INTEGER',
  precision: 32,
  length: 4,
  scale: 0,
  radix: 10,
  nullable: 1,
  remarks: '',
  columnDefault: '100',
  sqlDataType: 4,
  sqlDatetimeSub: 0,
  charOctetLength: 4,
  ordinalPosition: 1,
  isNullable: 'YES',
  specificName: 'REFRESH_ITEMS_1',
} satisfies CommunityProcedureParameterList['items'][number];

describe('community explorer loaders', () => {
  it('loads a namespace snapshot in parallel with exact scopes and one abort signal', async () => {
    const client = {
      listCommunityTables: vi.fn(async () => ({ items: [table] })),
      listCommunityViews: vi.fn(async () => ({ items: [view] })),
      listCommunityFunctions: vi.fn(async () => ({ items: [functionMetadata] })),
      listCommunityProcedures: vi.fn(async () => ({ items: [procedureMetadata] })),
      listCommunityTriggers: vi.fn(async () => ({ items: [triggerMetadata] })),
    } satisfies CommunityNamespaceClient;
    const signal = new AbortController().signal;

    const result = await loadCommunityNamespace(client, namespaceRequest, signal);

    expect(client.listCommunityTables).toHaveBeenCalledWith(
      { ...namespaceRequest, tableNamePattern: '%' },
      signal,
    );
    expect(client.listCommunityViews).toHaveBeenCalledWith(
      { ...namespaceRequest, viewNamePattern: '%' },
      signal,
    );
    expect(client.listCommunityFunctions).toHaveBeenCalledWith(namespaceRequest, signal);
    expect(client.listCommunityProcedures).toHaveBeenCalledWith(namespaceRequest, signal);
    expect(client.listCommunityTriggers).toHaveBeenCalledWith(namespaceRequest, signal);
    expect(result).toEqual({
      tables: [table],
      views: [view],
      functions: [functionMetadata],
      procedures: [procedureMetadata],
      triggers: [triggerMetadata],
      failures: [],
    });
  });

  it('keeps successful namespace groups when one long-tail service fails', async () => {
    const failure = new Error('functions unsupported');
    const client = {
      listCommunityTables: vi.fn(async () => ({ items: [table] })),
      listCommunityViews: vi.fn(async () => ({ items: [view] })),
      listCommunityFunctions: vi.fn(async () => { throw failure; }),
      listCommunityProcedures: vi.fn(async () => ({ items: [procedureMetadata] })),
      listCommunityTriggers: vi.fn(async () => ({ items: [triggerMetadata] })),
    } satisfies CommunityNamespaceClient;

    const result = await loadCommunityNamespace(client, namespaceRequest);

    expect(result.tables).toEqual([table]);
    expect(result.views).toEqual([view]);
    expect(result.functions).toEqual([]);
    expect(result.failures).toEqual([{ area: 'functions', error: failure }]);
  });

  it('publishes completed namespace groups while another long-tail request is pending', async () => {
    const functions = deferred<CommunityFunctionList>();
    const progress: CommunityNamespaceSnapshot[] = [];
    const client = {
      listCommunityTables: vi.fn(async () => ({ items: [table] })),
      listCommunityViews: vi.fn(async () => ({ items: [view] })),
      listCommunityFunctions: vi.fn(() => functions.promise),
      listCommunityProcedures: vi.fn(async () => ({ items: [procedureMetadata] })),
      listCommunityTriggers: vi.fn(async () => ({ items: [triggerMetadata] })),
    } satisfies CommunityNamespaceClient;

    const result = loadCommunityNamespace(
      client,
      namespaceRequest,
      undefined,
      (snapshot) => progress.push(snapshot),
    );

    await vi.waitFor(() => expect(progress.some((snapshot) => snapshot.tables.length === 1)).toBe(true));
    expect(progress.at(-1)?.functions).toEqual([]);
    functions.resolve({ items: [functionMetadata] });
    await expect(result).resolves.toMatchObject({ tables: [table], functions: [functionMetadata] });
  });

  it('loads every table detail collection with one exact request and signal', async () => {
    const client = {
      listCommunityColumns: vi.fn(async () => ({ items: [column] })),
      listCommunityIndexes: vi.fn(async () => ({ items: [index] })),
      listCommunityImportedKeys: vi.fn(async () => ({ items: [importedKey] })),
      listCommunityExportedKeys: vi.fn(async () => ({ items: [exportedKey] })),
      listCommunityPrimaryKeys: vi.fn(async () => ({ items: [primaryKey] })),
    } satisfies CommunityTableDetailClient;
    const request = { ...namespaceRequest, tableName: 'ITEMS' };
    const signal = new AbortController().signal;

    const result = await loadCommunityTableDetail(client, request, signal);

    for (const method of [
      client.listCommunityColumns,
      client.listCommunityIndexes,
      client.listCommunityImportedKeys,
      client.listCommunityExportedKeys,
      client.listCommunityPrimaryKeys,
    ]) {
      expect(method).toHaveBeenCalledWith(request, signal);
    }
    expect(result).toEqual({
      columns: [column],
      indexes: [index],
      importedKeys: [importedKey],
      exportedKeys: [exportedKey],
      primaryKeys: [primaryKey],
      failures: [],
    });
  });

  it('keeps table columns when relation metadata is unavailable', async () => {
    const failure = new Error('foreign keys unsupported');
    const client = {
      listCommunityColumns: vi.fn(async () => ({ items: [column] })),
      listCommunityIndexes: vi.fn(async () => ({ items: [index] })),
      listCommunityImportedKeys: vi.fn(async () => { throw failure; }),
      listCommunityExportedKeys: vi.fn(async () => ({ items: [exportedKey] })),
      listCommunityPrimaryKeys: vi.fn(async () => ({ items: [primaryKey] })),
    } satisfies CommunityTableDetailClient;

    const result = await loadCommunityTableDetail(client, {
      ...namespaceRequest,
      tableName: 'ITEMS',
    });

    expect(result.columns).toEqual([column]);
    expect(result.importedKeys).toEqual([]);
    expect(result.failures).toEqual([{ area: 'importedKeys', error: failure }]);
  });

  it('publishes table columns while optional relation metadata is pending', async () => {
    const importedKeys = deferred<CommunityForeignKeyList>();
    const progress: CommunityTableDetail[] = [];
    const client = {
      listCommunityColumns: vi.fn(async () => ({ items: [column] })),
      listCommunityIndexes: vi.fn(async () => ({ items: [index] })),
      listCommunityImportedKeys: vi.fn(() => importedKeys.promise),
      listCommunityExportedKeys: vi.fn(async () => ({ items: [exportedKey] })),
      listCommunityPrimaryKeys: vi.fn(async () => ({ items: [primaryKey] })),
    } satisfies CommunityTableDetailClient;

    const result = loadCommunityTableDetail(
      client,
      { ...namespaceRequest, tableName: 'ITEMS' },
      undefined,
      (detail) => progress.push(detail),
    );

    await vi.waitFor(() => expect(progress.some((detail) => detail.columns.length === 1)).toBe(true));
    expect(progress.at(-1)?.importedKeys).toEqual([]);
    importedKeys.resolve({ items: [importedKey] });
    await expect(result).resolves.toMatchObject({ columns: [column], importedKeys: [importedKey] });
  });

  it('combines function detail and parameters with the exact signal-bound request', async () => {
    const client = {
      getCommunityFunction: vi.fn(async () => functionMetadata),
      listCommunityFunctionParameters: vi.fn(async () => ({ items: [functionParameter] })),
    } satisfies CommunityFunctionDetailClient;
    const request = { ...namespaceRequest, functionName: 'DOUBLE_VALUE' };
    const signal = new AbortController().signal;

    await expect(loadCommunityFunctionDetail(client, request, signal)).resolves.toEqual({
      function: functionMetadata,
      parameters: [functionParameter],
      failures: [],
    });
    expect(client.getCommunityFunction).toHaveBeenCalledWith(request, signal);
    expect(client.listCommunityFunctionParameters).toHaveBeenCalledWith(request, signal);
  });

  it('combines procedure detail and parameters with the exact signal-bound request', async () => {
    const client = {
      getCommunityProcedure: vi.fn(async () => procedureMetadata),
      listCommunityProcedureParameters: vi.fn(async () => ({ items: [procedureParameter] })),
    } satisfies CommunityProcedureDetailClient;
    const request = { ...namespaceRequest, procedureName: 'REFRESH_ITEMS' };
    const signal = new AbortController().signal;

    await expect(loadCommunityProcedureDetail(client, request, signal)).resolves.toEqual({
      procedure: procedureMetadata,
      parameters: [procedureParameter],
      failures: [],
    });
    expect(client.getCommunityProcedure).toHaveBeenCalledWith(request, signal);
    expect(client.listCommunityProcedureParameters).toHaveBeenCalledWith(request, signal);
  });

  it('loads trigger detail with the exact signal-bound request', async () => {
    const client = {
      getCommunityTrigger: vi.fn(async () => triggerMetadata),
    } satisfies CommunityTriggerDetailClient;
    const request = { ...namespaceRequest, triggerName: 'ITEMS_AUDIT' };
    const signal = new AbortController().signal;

    await expect(loadCommunityTriggerDetail(client, request, signal)).resolves.toEqual({
      trigger: triggerMetadata,
    });
    expect(client.getCommunityTrigger).toHaveBeenCalledWith(request, signal);
  });
});

describe('selectCommunityPlugin', () => {
  const plugins = [
    {
      databaseType: 'H2',
      name: 'H2',
      behavior: {
        supportsDatabase: true,
        supportsSchema: true,
        preservesScriptBatchExecution: false,
      },
      drivers: [{
        url: 'jdbc:h2:mem:test',
        jdbcDriver: 'h2.jar',
        jdbcDriverClass: 'org.h2.Driver',
        downloadUrls: [],
        custom: false,
        defaultDriver: true,
      }],
      services: {
        metadataAvailable: true,
        sqlBuilderAvailable: true,
        sqlParserAvailable: true,
        dmlBuilderAvailable: true,
        valueProcessorAvailable: true,
        identifierProcessorAvailable: true,
      },
    },
    {
      databaseType: 'MYSQL',
      name: 'MySQL',
      behavior: {
        supportsDatabase: true,
        supportsSchema: false,
        preservesScriptBatchExecution: false,
      },
      drivers: [{
        url: 'jdbc:mysql://localhost:3306/',
        jdbcDriver: 'mysql.jar',
        jdbcDriverClass: 'com.mysql.cj.jdbc.Driver',
        downloadUrls: [],
        custom: false,
        defaultDriver: true,
      }],
      services: {
        metadataAvailable: true,
        sqlBuilderAvailable: true,
        sqlParserAvailable: true,
        dmlBuilderAvailable: true,
        valueProcessorAvailable: true,
        identifierProcessorAvailable: true,
      },
    },
  ] satisfies CommunityPluginCatalog['plugins'];
  const installedDrivers = [{
    artifactBytes: '100',
    artifactCount: 1,
    driverClass: 'com.mysql.cj.jdbc.Driver',
    driverId: 'sha256:mysql',
    name: 'MySQL',
    packId: 'mysql',
    version: '8',
  }] satisfies JdbcDriverList['items'];

  it('maps the datasource driver identity through the installed driver class', () => {
    expect(selectCommunityPlugin(plugins, installedDrivers, 'sha256:mysql')?.databaseType)
      .toBe('MYSQL');
  });

  it('preserves an explicit supported plugin choice', () => {
    expect(selectCommunityPlugin(plugins, installedDrivers, 'sha256:mysql', 'H2')?.databaseType)
      .toBe('H2');
  });

  it('does not guess a dialect when the datasource driver identity is unavailable', () => {
    expect(selectCommunityPlugin(plugins, [], 'sha256:missing')).toBeUndefined();
  });
});

describe('selectCommunityItem', () => {
  const items = [
    { id: 'system-id', name: 'SYSTEM', system: true },
    { id: 'app-id', name: 'APP', system: false },
    { id: 'audit-id', name: 'AUDIT', system: false },
  ];

  it('preserves a current item selected by id or name', () => {
    expect(selectCommunityItem(items, 'system-id')).toBe(items[0]);
    expect(selectCommunityItem(items, 'AUDIT')).toBe(items[2]);
  });

  it('falls back deterministically to a non-system item, then the first item', () => {
    expect(selectCommunityItem(items, 'missing')).toBe(items[1]);

    const systemOnly = items.filter((item) => item.system);
    expect(selectCommunityItem(systemOnly, null)).toBe(systemOnly[0]);
    expect(selectCommunityItem([], undefined)).toBeUndefined();
  });
});
