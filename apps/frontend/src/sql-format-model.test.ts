import { describe, expect, it } from 'vitest';

import { isCurrentSqlFormatRequest } from './sql-format-model';

const scope = {
  datasourceKey: 'datasource-1:3:h2',
  databaseType: 'H2',
  sql: 'select 1',
};

describe('Community SQL format request identity', () => {
  it('accepts only the latest request for the unchanged editor scope', () => {
    expect(isCurrentSqlFormatRequest({ sequence: 3, scope }, 3, { ...scope })).toBe(true);
    expect(isCurrentSqlFormatRequest({ sequence: 2, scope }, 3, { ...scope })).toBe(false);
  });

  it.each([
    ['SQL', { ...scope, sql: 'select 2' }],
    ['datasource', { ...scope, datasourceKey: 'datasource-2:1:h2' }],
    ['database type', { ...scope, databaseType: 'PostgreSQL' }],
  ])('rejects a response after the %s changes', (_label, currentScope) => {
    expect(isCurrentSqlFormatRequest(
      { sequence: 3, scope },
      3,
      currentScope,
    )).toBe(false);
  });
});
