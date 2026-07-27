import { describe, expect, it } from 'vitest';

import type { CommunityDatabase } from './backend';
import {
  buildCommunityNamespaceRequest,
  createCommunityNamespaceDraft,
  isCurrentCommunityNamespaceRequest,
} from './community-namespace-model';

const database = {
  name: 'inventory',
  comment: 'Inventory data',
  charset: 'utf8mb4',
  collation: 'utf8mb4_bin',
  owner: 'app',
  system: false,
} satisfies CommunityDatabase;

function request(kind: Parameters<typeof createCommunityNamespaceDraft>[2], patch = {}) {
  const draft = createCommunityNamespaceDraft(database, 'APP', kind);
  return buildCommunityNamespaceRequest('MYSQL', { ...draft, ...patch });
}

describe('Community namespace request model', () => {
  it('builds every closed database operation without a raw SQL field', () => {
    expect(request('createDatabase', { databaseName: ' archive ' })).toEqual({
      ok: true,
      request: {
        databaseType: 'MYSQL',
        operation: {
          kind: 'createDatabase',
          database: {
            name: 'archive',
            comment: '',
            charset: '',
            collation: '',
            owner: '',
            system: false,
          },
        },
      },
    });
    expect(request('alterDatabase', { newDatabaseName: 'inventory_v2' })).toMatchObject({
      ok: true,
      request: { operation: {
        kind: 'alterDatabase',
        oldDatabase: { name: 'inventory' },
        newDatabase: { name: 'inventory_v2' },
      } },
    });
    expect(request('dropDatabase')).toEqual({
      ok: true,
      request: { databaseType: 'MYSQL', operation: { kind: 'dropDatabase', databaseName: 'inventory' } },
    });
    expect(request('useDatabase')).toEqual({
      ok: true,
      request: { databaseType: 'MYSQL', operation: { kind: 'useDatabase', databaseName: 'inventory' } },
    });
    expect(JSON.stringify(request('createDatabase', { databaseName: 'archive' }))).not.toContain('sql');
  });

  it('builds every closed schema operation', () => {
    expect(request('createSchema', { schemaName: ' reporting ' })).toEqual({
      ok: true,
      request: {
        databaseType: 'MYSQL',
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
      },
    });
    expect(request('alterSchema', { newSchemaName: 'APP_V2' })).toEqual({
      ok: true,
      request: {
        databaseType: 'MYSQL',
        operation: { kind: 'alterSchema', oldSchemaName: 'APP', newSchemaName: 'APP_V2' },
      },
    });
    expect(request('dropSchema')).toEqual({
      ok: true,
      request: { databaseType: 'MYSQL', operation: { kind: 'dropSchema', schemaName: 'APP' } },
    });
  });

  it('rejects missing or oversized identifiers before transport', () => {
    expect(request('createDatabase')).toEqual({ ok: false, error: 'Database name is required.' });
    expect(request('alterSchema')).toEqual({ ok: false, error: 'New schema name is required.' });
    expect(request('createDatabase', { databaseName: '\u6570'.repeat(171) })).toEqual({
      ok: false,
      error: 'Database name exceeds the Community identifier limit.',
    });
    expect(request('createDatabase', { databaseName: 'archive; DROP DATABASE inventory' })).toEqual({
      ok: false,
      error: 'Database name contains unsupported characters.',
    });
    expect(request('createDatabase', { databaseName: 'archive', owner: 'app--admin' })).toEqual({
      ok: false,
      error: 'Owner contains unsupported characters.',
    });
    expect(request('createDatabase', { databaseName: 'archive', owner: 'app admin' })).toEqual({
      ok: false,
      error: 'Owner contains unsupported characters.',
    });
    expect(request('createDatabase', {
      databaseName: 'archive',
      comment: "curated'; DROP DATABASE inventory; --",
    })).toEqual({
      ok: false,
      error: 'Comment contains unsupported characters.',
    });
  });

  it('matches both request sequence and namespace scope', () => {
    const identity = { sequence: 7, scope: 'source-1:1:MYSQL:inventory:APP:3' };
    expect(isCurrentCommunityNamespaceRequest(identity, 7, identity.scope)).toBe(true);
    expect(isCurrentCommunityNamespaceRequest(identity, 8, identity.scope)).toBe(false);
    expect(isCurrentCommunityNamespaceRequest(identity, 7, null)).toBe(false);
    expect(isCurrentCommunityNamespaceRequest(identity, 7, 'source-1:1:MYSQL:archive:APP:3')).toBe(false);
  });
});
