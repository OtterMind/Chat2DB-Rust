import { describe, expect, it } from 'vitest';

import type {
  BuildCommunityCreateSchemaRequest,
  CommunityBuiltSql,
  CommunityDatabaseList,
  CommunityForeignKeyList,
  CommunityPluginCatalog,
  CommunityPrimaryKeyList,
  CommunitySchemaList,
  CommunitySqlAnalysis,
  CommunityTableColumnList,
  CommunityTableIndexList,
  CommunityTableList,
  CommunityViewList,
  ListCommunityColumnsRequest,
  ListCommunityDatabasesRequest,
  ListCommunityIndexesRequest,
  ListCommunitySchemasRequest,
  ListCommunityTableKeysRequest,
  ListCommunityTablesRequest,
  ListCommunityViewsRequest,
  ParseCommunitySqlRequest,
} from './client';
import { HttpBackendClient } from './http';
import { TauriBackendClient } from './tauri';

const listSchemasRequest = {
  datasourceId: 'datasource-1',
  databaseType: 'H2',
  databaseName: 'inventory',
} satisfies ListCommunitySchemasRequest;

const schema = {
  databaseName: 'inventory',
  name: 'reporting',
  comment: 'Reporting objects',
  owner: 'app',
  system: false,
};

const listDatabasesRequest = {
  datasourceId: 'datasource-1',
  databaseType: 'H2',
} satisfies ListCommunityDatabasesRequest;

const listTablesRequest = {
  datasourceId: 'datasource-1',
  databaseType: 'H2',
  databaseName: 'inventory',
  schemaName: 'APP',
  tableNamePattern: '%',
} satisfies ListCommunityTablesRequest;

const listColumnsRequest = {
  datasourceId: 'datasource-1',
  databaseType: 'H2',
  databaseName: 'inventory',
  schemaName: 'APP',
  tableName: 'ITEMS',
} satisfies ListCommunityColumnsRequest;

const listIndexesRequest = {
  ...listColumnsRequest,
} satisfies ListCommunityIndexesRequest;

const listViewsRequest = {
  datasourceId: 'datasource-1',
  databaseType: 'H2',
  databaseName: 'inventory',
  schemaName: 'APP',
  viewNamePattern: '%',
} satisfies ListCommunityViewsRequest;

const listKeysRequest = {
  ...listColumnsRequest,
} satisfies ListCommunityTableKeysRequest;

const buildSchemaRequest = {
  databaseType: 'H2',
  schema,
} satisfies BuildCommunityCreateSchemaRequest;

const parseSqlRequest = {
  databaseType: 'H2',
  sql: 'select 1',
} satisfies ParseCommunitySqlRequest;

const catalog = {
  sourceCommit: 'f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7',
  plugins: [{
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
    },
  }],
} satisfies CommunityPluginCatalog;

const schemas = { items: [schema] } satisfies CommunitySchemaList;
const databases = {
  items: [{
    name: 'inventory',
    comment: '',
    charset: '',
    collation: '',
    owner: '',
    system: false,
  }],
} satisfies CommunityDatabaseList;
const tables = {
  items: [{
    databaseName: 'inventory',
    schemaName: 'APP',
    name: 'ITEMS',
    tableType: 'BASE TABLE',
    comment: '',
    databaseType: '',
    pinned: false,
    ddl: '',
    engine: '',
    charset: '',
    collation: '',
    incrementValue: '9007199254740993',
    partition: '',
    tablespace: '',
    rows: '9007199254740994',
    dataLength: '9223372036854775807',
    createTime: '',
    updateTime: '',
  }],
} satisfies CommunityTableList;
const columns = {
  items: [{
    databaseName: 'inventory',
    schemaName: 'APP',
    tableName: 'ITEMS',
    name: 'ID',
    columnType: 'BIGINT',
    defaultValue: '',
    comment: '',
    primaryKeyName: 'CONSTRAINT_4',
    primaryKeyOrder: 1,
    extent: '',
    charset: '',
    collation: '',
    unit: '',
    defaultConstraintName: '',
  }],
} satisfies CommunityTableColumnList;
const indexes = {
  items: [{
    databaseName: 'inventory',
    schemaName: 'APP',
    tableName: 'ITEMS',
    name: 'IDX_ITEMS_LABEL',
    indexType: 'BTREE',
    comment: '',
    columns: [{
      databaseName: 'inventory',
      schemaName: 'APP',
      tableName: 'ITEMS',
      indexName: 'IDX_ITEMS_LABEL',
      columnName: 'LABEL',
      columnType: '',
      comment: '',
      collation: '',
      indexQualifier: 'inventory',
      sortOrder: 'A',
      cardinality: '9007199254740995',
      pages: '9007199254740996',
      filterCondition: '',
      subPart: '9007199254740997',
    }],
    method: '',
    foreignSchemaName: '',
    foreignTableName: '',
    foreignColumnNames: [],
  }],
} satisfies CommunityTableIndexList;
const views = {
  items: [{
    ...tables.items[0],
    name: 'ITEMS_VIEW',
    tableType: 'VIEW',
  }],
} satisfies CommunityViewList;
const foreignKeys = {
  items: [{
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
    primaryKeyName: 'CONSTRAINT_PARENT',
    deferrability: 7,
  }],
} satisfies CommunityForeignKeyList;
const primaryKeys = {
  items: [{
    databaseName: 'inventory',
    schemaName: 'APP',
    tableName: 'ITEMS',
    columnName: 'ID',
    name: 'CONSTRAINT_ITEMS',
  }],
} satisfies CommunityPrimaryKeyList;
const builtSql = { sql: 'CREATE SCHEMA reporting' } satisfies CommunityBuiltSql;
const analysis = {
  isSelect: true,
  statements: [{ sql: 'select 1', statementType: 'SELECT', kind: 'Select' }],
} satisfies CommunitySqlAnalysis;

const responses = [
  catalog,
  schemas,
  databases,
  tables,
  columns,
  indexes,
  views,
  foreignKeys,
  foreignKeys,
  primaryKeys,
  builtSql,
  analysis,
] as const;

function jsonResponse(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('Community backend adapter parity', () => {
  it('maps Community HTTP routes without reshaping payloads or decimal integers', async () => {
    const calls: Array<{ input: string; init?: RequestInit }> = [];
    let responseIndex = 0;
    const client = new HttpBackendClient({
      baseUrl: 'http://127.0.0.1:10825/',
      fetch: async (input, init) => {
        calls.push({ input: String(input), init });
        const response = responses[responseIndex];
        responseIndex += 1;
        if (response === undefined) throw new Error('unexpected request');
        return jsonResponse(response);
      },
    });

    const received = [
      await client.listCommunityPlugins(),
      await client.listCommunitySchemas(listSchemasRequest),
      await client.listCommunityDatabases(listDatabasesRequest),
      await client.listCommunityTables(listTablesRequest),
      await client.listCommunityColumns(listColumnsRequest),
      await client.listCommunityIndexes(listIndexesRequest),
      await client.listCommunityViews(listViewsRequest),
      await client.listCommunityImportedKeys(listKeysRequest),
      await client.listCommunityExportedKeys(listKeysRequest),
      await client.listCommunityPrimaryKeys(listKeysRequest),
      await client.buildCommunityCreateSchema(buildSchemaRequest),
      await client.parseCommunitySql(parseSqlRequest),
    ];

    expect(received).toEqual(responses);
    expect(JSON.stringify(received[3])).toContain('"rows":"9007199254740994"');
    expect(JSON.stringify(received[5])).toContain('"cardinality":"9007199254740995"');
    expect(calls.map(({ input, init }) => ({
      path: new URL(input).pathname,
      method: init?.method,
      body: init?.body === undefined ? undefined : JSON.parse(String(init.body)),
    }))).toEqual([
      { path: '/api/v1/community/plugins', method: 'GET', body: undefined },
      {
        path: '/api/v1/community/metadata/schemas',
        method: 'POST',
        body: listSchemasRequest,
      },
      {
        path: '/api/v1/community/metadata/databases',
        method: 'POST',
        body: listDatabasesRequest,
      },
      {
        path: '/api/v1/community/metadata/tables',
        method: 'POST',
        body: listTablesRequest,
      },
      {
        path: '/api/v1/community/metadata/columns',
        method: 'POST',
        body: listColumnsRequest,
      },
      {
        path: '/api/v1/community/metadata/indexes',
        method: 'POST',
        body: listIndexesRequest,
      },
      {
        path: '/api/v1/community/metadata/views',
        method: 'POST',
        body: listViewsRequest,
      },
      {
        path: '/api/v1/community/metadata/imported-keys',
        method: 'POST',
        body: listKeysRequest,
      },
      {
        path: '/api/v1/community/metadata/exported-keys',
        method: 'POST',
        body: listKeysRequest,
      },
      {
        path: '/api/v1/community/metadata/primary-keys',
        method: 'POST',
        body: listKeysRequest,
      },
      {
        path: '/api/v1/community/sql/build-create-schema',
        method: 'POST',
        body: buildSchemaRequest,
      },
      { path: '/api/v1/community/sql/parse', method: 'POST', body: parseSqlRequest },
    ]);
  });

  it('maps Tauri commands without reshaping payloads or decimal integers', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const client = new TauriBackendClient({
      invoke: async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
        calls.push({ command, args });
        const response = responses[calls.length - 1];
        if (response === undefined) throw new Error('unexpected request');
        return response as T;
      },
    });

    const received = [
      await client.listCommunityPlugins(),
      await client.listCommunitySchemas(listSchemasRequest),
      await client.listCommunityDatabases(listDatabasesRequest),
      await client.listCommunityTables(listTablesRequest),
      await client.listCommunityColumns(listColumnsRequest),
      await client.listCommunityIndexes(listIndexesRequest),
      await client.listCommunityViews(listViewsRequest),
      await client.listCommunityImportedKeys(listKeysRequest),
      await client.listCommunityExportedKeys(listKeysRequest),
      await client.listCommunityPrimaryKeys(listKeysRequest),
      await client.buildCommunityCreateSchema(buildSchemaRequest),
      await client.parseCommunitySql(parseSqlRequest),
    ];

    expect(received).toEqual(responses);
    expect(JSON.stringify(received[3])).toContain('"dataLength":"9223372036854775807"');
    expect(JSON.stringify(received[5])).toContain('"subPart":"9007199254740997"');
    expect(calls).toEqual([
      { command: 'list_community_plugins', args: undefined },
      { command: 'list_community_schemas', args: { request: listSchemasRequest } },
      { command: 'list_community_databases', args: { request: listDatabasesRequest } },
      { command: 'list_community_tables', args: { request: listTablesRequest } },
      { command: 'list_community_columns', args: { request: listColumnsRequest } },
      { command: 'list_community_indexes', args: { request: listIndexesRequest } },
      { command: 'list_community_views', args: { request: listViewsRequest } },
      { command: 'list_community_imported_keys', args: { request: listKeysRequest } },
      { command: 'list_community_exported_keys', args: { request: listKeysRequest } },
      { command: 'list_community_primary_keys', args: { request: listKeysRequest } },
      { command: 'build_community_create_schema', args: { request: buildSchemaRequest } },
      { command: 'parse_community_sql', args: { request: parseSqlRequest } },
    ]);
  });
});
