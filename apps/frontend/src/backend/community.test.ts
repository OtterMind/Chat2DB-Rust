import { describe, expect, it } from 'vitest';

import type {
  BuildCommunityCreateSchemaRequest,
  BuildCommunityDmlRequest,
  BuildCommunityNamespaceSqlRequest,
  CommunityBuiltSql,
  CommunityDatabaseList,
  CommunityFormattedSql,
  CommunityForeignKeyList,
  CommunityFunction,
  CommunityFunctionList,
  CommunityFunctionParameterList,
  CommunityPluginCatalog,
  CommunityPrimaryKeyList,
  CommunityProcedure,
  CommunityProcedureList,
  CommunityProcedureParameterList,
  CommunitySchemaList,
  CommunitySqlAnalysis,
  CommunitySqlCompletion,
  CommunitySqlValidation,
  CommunityTableColumnList,
  CommunityTableIndexList,
  CommunityTableList,
  CommunityTrigger,
  CommunityTriggerList,
  CommunityViewList,
  GetCommunityFunctionRequest,
  GetCommunityProcedureRequest,
  GetCommunityTriggerRequest,
  CompleteCommunitySqlRequest,
  FormatCommunitySqlRequest,
  ListCommunityColumnsRequest,
  ListCommunityDatabasesRequest,
  ListCommunityFunctionsRequest,
  ListCommunityIndexesRequest,
  ListCommunityProceduresRequest,
  ListCommunitySchemasRequest,
  ListCommunityTableKeysRequest,
  ListCommunityTablesRequest,
  ListCommunityTriggersRequest,
  ListCommunityViewsRequest,
  ParseCommunitySqlRequest,
  ValidateCommunitySqlRequest,
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

const listFunctionsRequest = {
  datasourceId: 'datasource-1',
  databaseType: 'H2',
  databaseName: 'inventory',
  schemaName: 'APP',
} satisfies ListCommunityFunctionsRequest;

const getFunctionRequest = {
  ...listFunctionsRequest,
  functionName: 'DOUBLE_VALUE',
} satisfies GetCommunityFunctionRequest;

const listProceduresRequest = {
  ...listFunctionsRequest,
} satisfies ListCommunityProceduresRequest;

const getProcedureRequest = {
  ...listProceduresRequest,
  procedureName: 'REFRESH_ITEMS',
} satisfies GetCommunityProcedureRequest;

const listTriggersRequest = {
  ...listFunctionsRequest,
} satisfies ListCommunityTriggersRequest;

const getTriggerRequest = {
  ...listTriggersRequest,
  triggerName: 'ITEMS_AUDIT',
} satisfies GetCommunityTriggerRequest;

const buildSchemaRequest = {
  databaseType: 'H2',
  schema,
} satisfies BuildCommunityCreateSchemaRequest;

const buildNamespaceRequest = {
  databaseType: 'H2',
  operation: {
    kind: 'createSchema',
    schema,
  },
} satisfies BuildCommunityNamespaceSqlRequest;

const parseSqlRequest = {
  databaseType: 'H2',
  sql: 'select 1',
} satisfies ParseCommunitySqlRequest;

const buildDmlRequest = {
  databaseType: 'H2',
  target: { databaseName: 'inventory', schemaName: 'APP', tableName: 'ITEMS' },
  statement: {
    kind: 'singleInsert',
    columns: [{ name: 'LABEL', dataTypeName: 'VARCHAR', precision: 255, scale: 0 }],
    row: { values: [{ kind: 'string', value: "O'Brien" }] },
  },
} satisfies BuildCommunityDmlRequest;

const validateSqlRequest = {
  databaseType: 'H2',
  sql: 'select from',
} satisfies ValidateCommunitySqlRequest;

const formatSqlRequest = {
  databaseType: 'H2',
  sql: 'select 1',
} satisfies FormatCommunitySqlRequest;

const completeSqlRequest = {
  datasourceId: 'datasource-1',
  databaseType: 'H2',
  databaseName: 'inventory',
  schemaName: 'APP',
  sql: 'select * fr',
  cursorUtf16: 11,
  minPrefixLength: 0,
  needFullName: false,
  keywordCase: 'UPPER',
  activeSnippetSlot: {
    type: 'SELECT_FUNCTION',
    replaceStartUtf16: 7,
    replaceEndUtf16: 11,
  },
} satisfies CompleteCommunitySqlRequest;

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
      dmlBuilderAvailable: true,
      valueProcessorAvailable: true,
      identifierProcessorAvailable: true,
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
const functions = {
  items: [{
    databaseName: 'inventory',
    schemaName: 'APP',
    name: 'DOUBLE_VALUE',
    remarks: '',
    functionType: 1,
    specificName: 'DOUBLE_VALUE_1',
    body: 'return value * 2;',
    template: '',
  }],
} satisfies CommunityFunctionList;
const functionDetail = functions.items[0] satisfies CommunityFunction;
const functionParameters = {
  items: [{
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
  }],
} satisfies CommunityFunctionParameterList;
const procedures = {
  items: [{
    databaseName: 'inventory',
    schemaName: 'APP',
    name: 'REFRESH_ITEMS',
    remarks: '',
    procedureType: 1,
    specificName: 'REFRESH_ITEMS_1',
    body: 'call refresh_items();',
  }],
} satisfies CommunityProcedureList;
const procedure = procedures.items[0] satisfies CommunityProcedure;
const procedureParameters = {
  items: [{
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
  }],
} satisfies CommunityProcedureParameterList;
const triggers = {
  items: [{
    databaseName: 'inventory',
    schemaName: 'APP',
    name: 'ITEMS_AUDIT',
    eventManipulation: 'INSERT',
    body: 'audit.ItemsTrigger',
  }],
} satisfies CommunityTriggerList;
const trigger = triggers.items[0] satisfies CommunityTrigger;
const builtSql = { sql: 'CREATE SCHEMA reporting' } satisfies CommunityBuiltSql;
const builtNamespaceSql = { sql: 'CREATE SCHEMA reporting' } satisfies CommunityBuiltSql;
const builtDml = { sql: "INSERT INTO inventory.APP.ITEMS (LABEL) VALUES ('O''Brien')" } satisfies CommunityBuiltSql;
const analysis = {
  isSelect: true,
  statements: [{ sql: 'select 1', statementType: 'SELECT', kind: 'Select' }],
} satisfies CommunitySqlAnalysis;
const validation = {
  valid: false,
  statements: [],
  diagnostics: [{
    startLine: 1,
    startColumn: 8,
    endLine: 1,
    endColumn: 12,
    tokenText: 'from',
    message: 'unexpected FROM',
  }],
} satisfies CommunitySqlValidation;
const formattedSql = {
  sql: 'SELECT\n  1',
} satisfies CommunityFormattedSql;
const completion = {
  status: 'SUCCESS',
  replaceStartUtf16: 9,
  replaceEndUtf16: 11,
  candidates: [{
    id: 'keyword:FROM',
    label: 'FROM',
    type: 'KEYWORD',
    insertText: 'FROM',
    insertType: 'PLAIN_TEXT',
    detail: 'keyword',
    description: 'SQL keyword',
    dataType: '',
    objectType: '',
    comment: '',
    datasourceName: '',
    databaseName: 'inventory',
    schemaName: 'APP',
    tableName: '',
    tableAlias: '',
    columnName: '',
    objectName: '',
    sortText: 'FROM',
    snippetSlots: [],
  }],
  editorHints: [{
    type: 'INSERT_VALUE',
    statementRange: {
      startLineNumber: 1,
      startColumn: 1,
      endLineNumber: 1,
      endColumn: 12,
    },
    items: [{
      rowIndex: 0,
      columnIndex: 0,
      fieldName: 'ID',
      fieldType: 'BIGINT',
      label: 'ID',
      active: true,
    }],
  }],
} satisfies CommunitySqlCompletion;

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
  functions,
  functionDetail,
  functionParameters,
  procedures,
  procedure,
  procedureParameters,
  triggers,
  trigger,
  builtSql,
  builtNamespaceSql,
  builtDml,
  analysis,
  validation,
  formattedSql,
  completion,
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
      await client.listCommunityFunctions(listFunctionsRequest),
      await client.getCommunityFunction(getFunctionRequest),
      await client.listCommunityFunctionParameters(getFunctionRequest),
      await client.listCommunityProcedures(listProceduresRequest),
      await client.getCommunityProcedure(getProcedureRequest),
      await client.listCommunityProcedureParameters(getProcedureRequest),
      await client.listCommunityTriggers(listTriggersRequest),
      await client.getCommunityTrigger(getTriggerRequest),
      await client.buildCommunityCreateSchema(buildSchemaRequest),
      await client.buildCommunityNamespaceSql(buildNamespaceRequest),
      await client.buildCommunityDml(buildDmlRequest),
      await client.parseCommunitySql(parseSqlRequest),
      await client.validateCommunitySql(validateSqlRequest),
      await client.formatCommunitySql(formatSqlRequest),
      await client.completeCommunitySql(completeSqlRequest),
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
        path: '/api/v1/community/metadata/functions',
        method: 'POST',
        body: listFunctionsRequest,
      },
      {
        path: '/api/v1/community/metadata/function',
        method: 'POST',
        body: getFunctionRequest,
      },
      {
        path: '/api/v1/community/metadata/function-parameters',
        method: 'POST',
        body: getFunctionRequest,
      },
      {
        path: '/api/v1/community/metadata/procedures',
        method: 'POST',
        body: listProceduresRequest,
      },
      {
        path: '/api/v1/community/metadata/procedure',
        method: 'POST',
        body: getProcedureRequest,
      },
      {
        path: '/api/v1/community/metadata/procedure-parameters',
        method: 'POST',
        body: getProcedureRequest,
      },
      {
        path: '/api/v1/community/metadata/triggers',
        method: 'POST',
        body: listTriggersRequest,
      },
      {
        path: '/api/v1/community/metadata/trigger',
        method: 'POST',
        body: getTriggerRequest,
      },
      {
        path: '/api/v1/community/sql/build-create-schema',
        method: 'POST',
        body: buildSchemaRequest,
      },
      {
        path: '/api/v1/community/sql/build-namespace',
        method: 'POST',
        body: buildNamespaceRequest,
      },
      { path: '/api/v1/community/sql/build-dml', method: 'POST', body: buildDmlRequest },
      { path: '/api/v1/community/sql/parse', method: 'POST', body: parseSqlRequest },
      { path: '/api/v1/community/sql/validate', method: 'POST', body: validateSqlRequest },
      { path: '/api/v1/community/sql/format', method: 'POST', body: formatSqlRequest },
      { path: '/api/v1/community/sql/complete', method: 'POST', body: completeSqlRequest },
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
      await client.listCommunityFunctions(listFunctionsRequest),
      await client.getCommunityFunction(getFunctionRequest),
      await client.listCommunityFunctionParameters(getFunctionRequest),
      await client.listCommunityProcedures(listProceduresRequest),
      await client.getCommunityProcedure(getProcedureRequest),
      await client.listCommunityProcedureParameters(getProcedureRequest),
      await client.listCommunityTriggers(listTriggersRequest),
      await client.getCommunityTrigger(getTriggerRequest),
      await client.buildCommunityCreateSchema(buildSchemaRequest),
      await client.buildCommunityNamespaceSql(buildNamespaceRequest),
      await client.buildCommunityDml(buildDmlRequest),
      await client.parseCommunitySql(parseSqlRequest),
      await client.validateCommunitySql(validateSqlRequest),
      await client.formatCommunitySql(formatSqlRequest),
      await client.completeCommunitySql(completeSqlRequest),
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
      { command: 'list_community_functions', args: { request: listFunctionsRequest } },
      { command: 'get_community_function', args: { request: getFunctionRequest } },
      {
        command: 'list_community_function_parameters',
        args: { request: getFunctionRequest },
      },
      { command: 'list_community_procedures', args: { request: listProceduresRequest } },
      { command: 'get_community_procedure', args: { request: getProcedureRequest } },
      {
        command: 'list_community_procedure_parameters',
        args: { request: getProcedureRequest },
      },
      { command: 'list_community_triggers', args: { request: listTriggersRequest } },
      { command: 'get_community_trigger', args: { request: getTriggerRequest } },
      { command: 'build_community_create_schema', args: { request: buildSchemaRequest } },
      { command: 'build_community_namespace_sql', args: { request: buildNamespaceRequest } },
      { command: 'build_community_dml', args: { request: buildDmlRequest } },
      { command: 'parse_community_sql', args: { request: parseSqlRequest } },
      { command: 'validate_community_sql', args: { request: validateSqlRequest } },
      { command: 'format_community_sql', args: { request: formatSqlRequest } },
      { command: 'complete_community_sql', args: { request: completeSqlRequest } },
    ]);
  });
});
