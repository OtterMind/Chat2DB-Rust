import type {
  BuildCommunityDmlRequest,
  CommunityPrimaryKeyList,
  CommunityTableColumnList,
} from './backend/client';

export type CommunityDmlMode = 'insert' | 'update';
export type CommunityDmlTableColumn = CommunityTableColumnList['items'][number];
export type CommunityDmlPrimaryKey = CommunityPrimaryKeyList['items'][number];
export type CommunityDmlTarget = BuildCommunityDmlRequest['target'];
export type CommunityDmlValueKind =
  | 'string'
  | 'decimal'
  | 'boolean'
  | 'date'
  | 'time'
  | 'localDatetime'
  | 'offsetDatetime'
  | 'binary';

type CommunityDmlStatement = BuildCommunityDmlRequest['statement'];
type CommunityDmlInsertStatement = Extract<CommunityDmlStatement, { kind: 'singleInsert' }>;
type CommunityDmlValue = CommunityDmlInsertStatement['row']['values'][number];

export interface CommunityDmlDraftValue {
  input: string;
  isNull: boolean;
}

export interface CommunityDmlInsertColumnDraft {
  column: CommunityDmlTableColumn;
  enabled: boolean;
}

export interface CommunityDmlInsertRowDraft {
  id: number;
  values: Record<string, CommunityDmlDraftValue>;
}

export interface CommunityDmlInsertDraft {
  columns: CommunityDmlInsertColumnDraft[];
  rows: CommunityDmlInsertRowDraft[];
}

export interface CommunityDmlUpdateFieldDraft {
  column: CommunityDmlTableColumn;
  enabled: boolean;
  value: CommunityDmlDraftValue;
}

export interface CommunityDmlUpdateDraft {
  assignments: CommunityDmlUpdateFieldDraft[];
  predicates: CommunityDmlUpdateFieldDraft[];
}

export type CommunityDmlBuildResult =
  | { ok: true; request: BuildCommunityDmlRequest }
  | { ok: false; error: string };

export interface CommunityDmlRequestIdentity {
  sequence: number;
  scope: string;
}

const MAX_COLUMNS = 2048;
const MAX_ROWS = 4096;
const MAX_CELLS = 32768;
const DECIMAL_PATTERN = /^-?\d+(?:\.\d+)?$/;
const BASE64_PATTERN = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
const BASE64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

function normalizedType(columnType: string): string {
  return columnType.trim().toUpperCase().replace(/\s+/g, ' ');
}

function isCanonicalBase64(value: string): boolean {
  if (value.length % 4 !== 0 || !BASE64_PATTERN.test(value)) return false;
  if (value.endsWith('==')) return BASE64_ALPHABET.indexOf(value.at(-3) ?? '') % 16 === 0;
  if (value.endsWith('=')) return BASE64_ALPHABET.indexOf(value.at(-2) ?? '') % 4 === 0;
  return true;
}

export function communityDmlValueKind(columnType: string): CommunityDmlValueKind {
  const type = normalizedType(columnType);
  if (/\b(BINARY|VARBINARY|LONGVARBINARY|BLOB|BYTEA|RAW|IMAGE|BIT VARYING)\b/.test(type)) return 'binary';
  if (/\b(BOOL|BOOLEAN|BIT)\b/.test(type)) return 'boolean';
  if (/\b(TIMESTAMP|DATETIME|TIMESTAMPTZ|DATETIMEOFFSET)\b/.test(type)) {
    return /\b(WITH TIME ZONE|TIMESTAMPTZ|DATETIMEOFFSET)\b/.test(type)
      ? 'offsetDatetime'
      : 'localDatetime';
  }
  if (/\bTIME\b/.test(type)) return 'time';
  if (/\bDATE\b/.test(type)) return 'date';
  if (/\b(TINYINT|SMALLINT|MEDIUMINT|BIGINT|INTEGER|INT|DECIMAL|DEC|NUMERIC|NUMBER|FLOAT|DOUBLE|REAL|MONEY|SERIAL)\b/.test(type)) {
    return 'decimal';
  }
  return 'string';
}

export function createCommunityDmlDraftValue(
  column: CommunityDmlTableColumn,
): CommunityDmlDraftValue {
  return {
    input: communityDmlValueKind(column.columnType) === 'boolean' ? 'false' : '',
    isNull: false,
  };
}

function isGeneratedColumn(column: CommunityDmlTableColumn): boolean {
  return column.autoIncrement === true || column.generatedColumn === true;
}

function createRow(
  columns: readonly CommunityDmlTableColumn[],
  id: number,
): CommunityDmlInsertRowDraft {
  return {
    id,
    values: Object.fromEntries(columns.map((column) => [
      column.name,
      createCommunityDmlDraftValue(column),
    ])),
  };
}

export function createCommunityDmlInsertDraft(
  columns: readonly CommunityDmlTableColumn[],
): CommunityDmlInsertDraft {
  const eligible = columns.filter((column) => !isGeneratedColumn(column));
  const enabledNames = new Set(eligible.map((column) => column.name));
  return {
    columns: columns.map((column) => ({ column, enabled: enabledNames.has(column.name) })),
    rows: [createRow(columns, 1)],
  };
}

function primaryKeyNames(
  columns: readonly CommunityDmlTableColumn[],
  primaryKeys: readonly CommunityDmlPrimaryKey[],
): Set<string> {
  return new Set([
    ...primaryKeys.map((key) => key.columnName.toUpperCase()),
    ...columns.filter((column) => column.primaryKey === true).map((column) => column.name.toUpperCase()),
  ]);
}

export function createCommunityDmlUpdateDraft(
  columns: readonly CommunityDmlTableColumn[],
  primaryKeys: readonly CommunityDmlPrimaryKey[],
): CommunityDmlUpdateDraft {
  const keyNames = primaryKeyNames(columns, primaryKeys);
  return {
    assignments: columns.map((column) => ({
      column,
      enabled: false,
      value: createCommunityDmlDraftValue(column),
    })),
    predicates: columns.map((column) => ({
      column,
      enabled: keyNames.has(column.name.toUpperCase()),
      value: createCommunityDmlDraftValue(column),
    })),
  };
}

export function addCommunityDmlInsertRow(
  draft: CommunityDmlInsertDraft,
  rowId: number,
): CommunityDmlInsertDraft {
  if (draft.rows.length >= MAX_ROWS) return draft;
  return {
    ...draft,
    rows: [...draft.rows, createRow(draft.columns.map(({ column }) => column), rowId)],
  };
}

function requestColumn(column: CommunityDmlTableColumn) {
  return {
    name: column.name,
    dataTypeName: column.columnType,
    ...(column.columnSize == null ? {} : { precision: column.columnSize }),
    ...(column.decimalDigits == null ? {} : { scale: column.decimalDigits }),
  };
}

function valueResult(
  column: CommunityDmlTableColumn,
  draft: CommunityDmlDraftValue,
  label: string,
): { ok: true; value: CommunityDmlValue } | { ok: false; error: string } {
  if (draft.isNull) return { ok: true, value: { kind: 'null' } };
  const kind = communityDmlValueKind(column.columnType);
  if (kind === 'string') return { ok: true, value: { kind: 'string', value: draft.input } };
  const input = draft.input.trim();
  if (kind === 'decimal') {
    return DECIMAL_PATTERN.test(input)
      ? { ok: true, value: { kind: 'decimal', value: input } }
      : { ok: false, error: `${label} must be a decimal value.` };
  }
  if (kind === 'boolean') {
    return input === 'true' || input === 'false'
      ? { ok: true, value: { kind: 'boolean', value: input === 'true' } }
      : { ok: false, error: `${label} must be true or false.` };
  }
  if (kind === 'binary') {
    return isCanonicalBase64(input)
      ? { ok: true, value: { kind: 'binary', base64: input } }
      : { ok: false, error: `${label} must be canonical Base64.` };
  }
  if (!input) return { ok: false, error: `${label} is required.` };
  return {
    ok: true,
    value: {
      kind: 'temporal',
      temporalKind: kind,
      value: input,
    },
  };
}

export function buildCommunityInsertRequest(
  databaseType: string,
  target: CommunityDmlTarget,
  draft: CommunityDmlInsertDraft,
): CommunityDmlBuildResult {
  const selected = draft.columns.filter(({ enabled }) => enabled);
  if (selected.length === 0) return { ok: false, error: 'Select at least one column.' };
  if (selected.length > MAX_COLUMNS) {
    return { ok: false, error: 'The INSERT exceeds the Community DML column limit.' };
  }
  if (draft.rows.length === 0) return { ok: false, error: 'Add at least one row.' };
  if (draft.rows.length > MAX_ROWS || selected.length * draft.rows.length > MAX_CELLS) {
    return { ok: false, error: 'The INSERT exceeds the Community DML row or cell limit.' };
  }
  const rows: CommunityDmlValue[][] = [];
  for (const [rowIndex, row] of draft.rows.entries()) {
    const values: CommunityDmlValue[] = [];
    for (const { column } of selected) {
      const result = valueResult(column, row.values[column.name], `Row ${rowIndex + 1} ${column.name}`);
      if (!result.ok) return result;
      values.push(result.value);
    }
    rows.push(values);
  }
  const columns = selected.map(({ column }) => requestColumn(column));
  return {
    ok: true,
    request: {
      databaseType,
      target,
      statement: rows.length === 1
        ? { kind: 'singleInsert', columns, row: { values: rows[0] } }
        : { kind: 'multiInsert', columns, rows: rows.map((values) => ({ values })) },
    },
  };
}

export function buildCommunityUpdateRequest(
  databaseType: string,
  target: CommunityDmlTarget,
  draft: CommunityDmlUpdateDraft,
): CommunityDmlBuildResult {
  const selectedAssignments = draft.assignments.filter(({ enabled }) => enabled);
  const selectedPredicates = draft.predicates.filter(({ enabled }) => enabled);
  if (selectedAssignments.length === 0) return { ok: false, error: 'Select at least one SET column.' };
  if (selectedPredicates.length === 0) {
    return { ok: false, error: 'Select at least one WHERE equality predicate.' };
  }
  if (selectedAssignments.length > MAX_COLUMNS || selectedPredicates.length > MAX_COLUMNS) {
    return { ok: false, error: 'The UPDATE exceeds the Community DML column limit.' };
  }

  const assignments = [];
  for (const field of selectedAssignments) {
    const result = valueResult(field.column, field.value, `SET ${field.column.name}`);
    if (!result.ok) return result;
    assignments.push({ column: requestColumn(field.column), value: result.value });
  }
  const predicates = [];
  for (const field of selectedPredicates) {
    if (field.value.isNull) {
      return { ok: false, error: `WHERE ${field.column.name} cannot be NULL.` };
    }
    const result = valueResult(field.column, field.value, `WHERE ${field.column.name}`);
    if (!result.ok) return result;
    predicates.push({ column: requestColumn(field.column), value: result.value });
  }
  return {
    ok: true,
    request: {
      databaseType,
      target,
      statement: { kind: 'update', assignments, predicates },
    },
  };
}

export function isCurrentCommunityDmlRequest(
  request: CommunityDmlRequestIdentity,
  latestSequence: number,
  currentScope: string | null,
): boolean {
  return request.sequence === latestSequence && request.scope === currentScope;
}
