import { describe, expect, it } from 'vitest';

import type {
  BuildCommunityCreateSchemaRequest,
  CommunityBuiltSql,
  CommunityPluginCatalog,
  CommunitySchemaList,
  CommunitySqlAnalysis,
  ListCommunitySchemasRequest,
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
const builtSql = { sql: 'CREATE SCHEMA reporting' } satisfies CommunityBuiltSql;
const analysis = {
  isSelect: true,
  statements: [{ sql: 'select 1', statementType: 'SELECT', kind: 'Select' }],
} satisfies CommunitySqlAnalysis;

const responses = [catalog, schemas, builtSql, analysis] as const;

function jsonResponse(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('Community backend adapter parity', () => {
  it('maps the shared methods to the four HTTP routes without reshaping payloads', async () => {
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
      await client.buildCommunityCreateSchema(buildSchemaRequest),
      await client.parseCommunitySql(parseSqlRequest),
    ];

    expect(received).toEqual(responses);
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
        path: '/api/v1/community/sql/build-create-schema',
        method: 'POST',
        body: buildSchemaRequest,
      },
      { path: '/api/v1/community/sql/parse', method: 'POST', body: parseSqlRequest },
    ]);
  });

  it('maps the same methods to snake-case Tauri commands without reshaping payloads', async () => {
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
      await client.buildCommunityCreateSchema(buildSchemaRequest),
      await client.parseCommunitySql(parseSqlRequest),
    ];

    expect(received).toEqual(responses);
    expect(calls).toEqual([
      { command: 'list_community_plugins', args: undefined },
      { command: 'list_community_schemas', args: { request: listSchemasRequest } },
      { command: 'build_community_create_schema', args: { request: buildSchemaRequest } },
      { command: 'parse_community_sql', args: { request: parseSqlRequest } },
    ]);
  });
});
