import { describe, expect, it } from 'vitest';

import type {
  CommunityPrimaryKeyList,
  CommunityTableColumnList,
} from './backend/client';
import {
  addCommunityDmlInsertRow,
  buildCommunityInsertRequest,
  buildCommunityUpdateRequest,
  communityDmlValueKind,
  createCommunityDmlInsertDraft,
  createCommunityDmlUpdateDraft,
  isCurrentCommunityDmlRequest,
} from './community-dml-model';

type Column = CommunityTableColumnList['items'][number];

function column(name: string, columnType: string, patch: Partial<Column> = {}): Column {
  return {
    databaseName: 'inventory',
    schemaName: 'APP',
    tableName: 'ITEMS',
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

const id = column('ID', 'BIGINT', {
  primaryKey: true,
  primaryKeyName: 'PK_ITEMS',
  primaryKeyOrder: 1,
  columnSize: 64,
  decimalDigits: 0,
});
const label = column('LABEL', 'VARCHAR', { columnSize: 255 });
const active = column('ACTIVE', 'BOOLEAN');
const createdAt = column('CREATED_AT', 'TIMESTAMP');
const payload = column('PAYLOAD', 'VARBINARY');
const generated = column('GENERATED_ID', 'BIGINT', { autoIncrement: true });
const columns = [id, label, active, createdAt, payload, generated];
const target = { databaseName: 'inventory', schemaName: 'APP', tableName: 'ITEMS' };
const primaryKeys = [{
  databaseName: 'inventory',
  schemaName: 'APP',
  tableName: 'ITEMS',
  columnName: 'ID',
  name: 'PK_ITEMS',
}] satisfies CommunityPrimaryKeyList['items'];

describe('Community DML value model', () => {
  it.each([
    ['VARCHAR', 'string'],
    ['DECIMAL(18, 2)', 'decimal'],
    ['BOOLEAN', 'boolean'],
    ['DATE', 'date'],
    ['TIME', 'time'],
    ['TIMESTAMP', 'localDatetime'],
    ['TIMESTAMP WITH TIME ZONE', 'offsetDatetime'],
    ['BYTEA', 'binary'],
  ] as const)('maps %s metadata to a typed value editor', (columnType, expected) => {
    expect(communityDmlValueKind(columnType)).toBe(expected);
  });

  it('keeps an empty string distinct from SQL NULL', () => {
    const draft = createCommunityDmlInsertDraft([label]);
    const empty = buildCommunityInsertRequest('H2', target, draft);
    expect(empty.ok && empty.request.statement).toEqual({
      kind: 'singleInsert',
      columns: [{ name: 'LABEL', dataTypeName: 'VARCHAR', precision: 255 }],
      row: { values: [{ kind: 'string', value: '' }] },
    });

    draft.rows[0].values.LABEL.isNull = true;
    const nullable = buildCommunityInsertRequest('H2', target, draft);
    expect(nullable.ok && nullable.request.statement).toMatchObject({
      row: { values: [{ kind: 'null' }] },
    });
  });
});

describe('Community INSERT drafts', () => {
  it('defaults generated columns off and maps one row to single INSERT', () => {
    const draft = createCommunityDmlInsertDraft(columns);
    expect(draft.columns.find(({ column: item }) => item.name === 'GENERATED_ID')?.enabled).toBe(false);
    draft.rows[0].values.ID.input = '42';
    draft.rows[0].values.LABEL.input = "O'Brien";
    draft.rows[0].values.ACTIVE.input = 'true';
    draft.rows[0].values.CREATED_AT.input = '2026-07-27T12:30:00';
    draft.rows[0].values.PAYLOAD.input = 'AQI=';

    const result = buildCommunityInsertRequest('H2', target, draft);

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.request.statement).toMatchObject({
      kind: 'singleInsert',
      row: { values: [
        { kind: 'decimal', value: '42' },
        { kind: 'string', value: "O'Brien" },
        { kind: 'boolean', value: true },
        { kind: 'temporal', temporalKind: 'localDatetime', value: '2026-07-27T12:30:00' },
        { kind: 'binary', base64: 'AQI=' },
      ] },
    });
  });

  it('keeps every generated column disabled when a table has no writable columns', () => {
    const draft = createCommunityDmlInsertDraft([
      generated,
      column('COMPUTED_VALUE', 'VARCHAR', { generatedColumn: true }),
    ]);

    expect(draft.columns.every(({ enabled }) => !enabled)).toBe(true);
    expect(buildCommunityInsertRequest('H2', target, draft)).toEqual({
      ok: false,
      error: 'Select at least one column.',
    });
  });

  it('maps multiple rows to a batch INSERT with stable column order', () => {
    let draft = createCommunityDmlInsertDraft([id, label]);
    draft.rows[0].values.ID.input = '1';
    draft.rows[0].values.LABEL.input = 'one';
    draft = addCommunityDmlInsertRow(draft, 2);
    draft.rows[1].values.ID.input = '2';
    draft.rows[1].values.LABEL.input = 'two';

    const result = buildCommunityInsertRequest('H2', target, draft);

    expect(result.ok && result.request.statement).toEqual({
      kind: 'multiInsert',
      columns: [
        { name: 'ID', dataTypeName: 'BIGINT', precision: 64, scale: 0 },
        { name: 'LABEL', dataTypeName: 'VARCHAR', precision: 255 },
      ],
      rows: [
        { values: [{ kind: 'decimal', value: '1' }, { kind: 'string', value: 'one' }] },
        { values: [{ kind: 'decimal', value: '2' }, { kind: 'string', value: 'two' }] },
      ],
    });
  });

  it('rejects invalid typed input before transport', () => {
    const decimal = createCommunityDmlInsertDraft([id]);
    expect(buildCommunityInsertRequest('H2', target, decimal)).toEqual({
      ok: false,
      error: 'Row 1 ID must be a decimal value.',
    });
    for (const unsupported of ['+1', '.5', '1.', '1e3']) {
      decimal.rows[0].values.ID.input = unsupported;
      expect(buildCommunityInsertRequest('H2', target, decimal)).toEqual({
        ok: false,
        error: 'Row 1 ID must be a decimal value.',
      });
    }

    const binary = createCommunityDmlInsertDraft([payload]);
    binary.rows[0].values.PAYLOAD.input = 'not base64';
    expect(buildCommunityInsertRequest('H2', target, binary)).toEqual({
      ok: false,
      error: 'Row 1 PAYLOAD must be canonical Base64.',
    });
    binary.rows[0].values.PAYLOAD.input = 'AB==';
    expect(buildCommunityInsertRequest('H2', target, binary)).toEqual({
      ok: false,
      error: 'Row 1 PAYLOAD must be canonical Base64.',
    });
  });

  it('rejects more than 2048 selected columns before transport', () => {
    const oversized = createCommunityDmlInsertDraft(
      Array.from({ length: 2049 }, (_, index) => column(`VALUE_${index}`, 'VARCHAR')),
    );
    expect(buildCommunityInsertRequest('H2', target, oversized)).toEqual({
      ok: false,
      error: 'The INSERT exceeds the Community DML column limit.',
    });
  });
});

describe('Community UPDATE drafts', () => {
  it('prefers primary keys for WHERE and requires explicit SET selection', () => {
    const draft = createCommunityDmlUpdateDraft(columns, primaryKeys);
    expect(draft.predicates.filter(({ enabled }) => enabled).map(({ column: item }) => item.name))
      .toEqual(['ID']);
    expect(draft.assignments.filter(({ enabled }) => enabled).map(({ column: item }) => item.name))
      .toEqual([]);
  });

  it('requires an ordered equality predicate and rejects NULL predicates', () => {
    const draft = createCommunityDmlUpdateDraft([id, label], primaryKeys);
    draft.assignments[1].enabled = true;
    draft.assignments[1].value.input = 'updated';
    draft.predicates[0].enabled = false;
    expect(buildCommunityUpdateRequest('H2', target, draft)).toEqual({
      ok: false,
      error: 'Select at least one WHERE equality predicate.',
    });

    draft.predicates[0].enabled = true;
    draft.predicates[0].value.isNull = true;
    expect(buildCommunityUpdateRequest('H2', target, draft)).toEqual({
      ok: false,
      error: 'WHERE ID cannot be NULL.',
    });
  });

  it('preserves metadata order in SET and WHERE assignments', () => {
    const draft = createCommunityDmlUpdateDraft([id, label, active], primaryKeys);
    draft.assignments[1].enabled = true;
    draft.assignments[2].enabled = true;
    draft.assignments[1].value.input = 'updated';
    draft.assignments[2].value.input = 'true';
    draft.predicates[0].value.input = '7';

    const result = buildCommunityUpdateRequest('H2', target, draft);

    expect(result.ok && result.request.statement).toMatchObject({
      kind: 'update',
      assignments: [
        { column: { name: 'LABEL' }, value: { kind: 'string', value: 'updated' } },
        { column: { name: 'ACTIVE' }, value: { kind: 'boolean', value: true } },
      ],
      predicates: [
        { column: { name: 'ID' }, value: { kind: 'decimal', value: '7' } },
      ],
    });
  });
});

describe('Community DML request identity', () => {
  it('rejects closed, superseded, and table-switched responses', () => {
    const request = { sequence: 3, scope: 'source-1:H2:inventory:APP:ITEMS' };
    expect(isCurrentCommunityDmlRequest(request, 3, request.scope)).toBe(true);
    expect(isCurrentCommunityDmlRequest(request, 4, request.scope)).toBe(false);
    expect(isCurrentCommunityDmlRequest(request, 3, null)).toBe(false);
    expect(isCurrentCommunityDmlRequest(request, 3, 'source-1:H2:inventory:APP:ORDERS')).toBe(false);
  });
});
