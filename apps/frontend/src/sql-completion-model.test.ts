import { describe, expect, it } from 'vitest';

import {
  applySqlCompletion,
  isCurrentSqlCompletionRequest,
  moveSqlCompletionSelection,
} from './sql-completion-model';

const scope = {
  datasourceKey: 'datasource-1:3:h2',
  databaseType: 'H2',
  databaseName: 'inventory',
  schemaName: 'APP',
  sql: 'select * fr',
  cursorUtf16: 11,
  refreshGeneration: 4,
};

describe('Community SQL completion request identity', () => {
  it('accepts only the latest request for the unchanged editor scope', () => {
    expect(isCurrentSqlCompletionRequest({ sequence: 7, scope }, 7, { ...scope })).toBe(true);
    expect(isCurrentSqlCompletionRequest({ sequence: 6, scope }, 7, { ...scope })).toBe(false);
  });

  it.each([
    ['SQL', { ...scope, sql: 'select * from' }],
    ['datasource', { ...scope, datasourceKey: 'datasource-2:1:h2' }],
    ['database type', { ...scope, databaseType: 'PostgreSQL' }],
    ['database', { ...scope, databaseName: 'warehouse' }],
    ['schema', { ...scope, schemaName: 'reporting' }],
    ['cursor', { ...scope, cursorUtf16: 8 }],
    ['refresh generation', { ...scope, refreshGeneration: 5 }],
  ])('rejects a response after the %s changes', (_label, currentScope) => {
    expect(isCurrentSqlCompletionRequest(
      { sequence: 7, scope },
      7,
      currentScope,
    )).toBe(false);
  });
});

describe('Community SQL completion edits', () => {
  it('uses the response range and places the caret after inserted text', () => {
    expect(applySqlCompletion('select * fr', 9, 11, { label: 'FROM', insertText: 'FROM' })).toEqual({
      sql: 'select * FROM',
      caret: 13,
    });
  });

  it('prefers a candidate-specific replacement range', () => {
    expect(applySqlCompletion('select * fr', 9, 11, {
      insertText: 'FROM items',
      label: 'items',
      replaceStartUtf16: 7,
      replaceEndUtf16: 11,
    })).toEqual({
      sql: 'select FROM items',
      caret: 17,
    });
  });

  it('keeps browser and Java UTF-16 offsets aligned around surrogate pairs', () => {
    const sql = "select '😀' fr";
    const replaceStart = sql.indexOf('fr');
    expect(replaceStart).toBe(12);
    expect(applySqlCompletion(sql, replaceStart, sql.length, { label: 'FROM' })).toEqual({
      sql: "select '😀' FROM",
      caret: 16,
    });
  });

  it.each([
    [-1, 2],
    [3, 2],
    [1, 99],
  ])('rejects an invalid replacement range %i..%i', (replaceStart, replaceEnd) => {
    expect(applySqlCompletion('abc', replaceStart, replaceEnd, { label: 'x', insertText: 'x' })).toBeNull();
  });
});

describe('Community SQL completion selection', () => {
  it('wraps keyboard navigation and handles an empty list', () => {
    expect(moveSqlCompletionSelection(0, 3, 1)).toBe(1);
    expect(moveSqlCompletionSelection(2, 3, 1)).toBe(0);
    expect(moveSqlCompletionSelection(0, 3, -1)).toBe(2);
    expect(moveSqlCompletionSelection(0, 0, 1)).toBe(-1);
  });
});
