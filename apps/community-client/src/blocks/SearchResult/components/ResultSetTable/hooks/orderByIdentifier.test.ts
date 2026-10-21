import {
  buildResultSetOrderByText,
  quoteDoubleQuotedIdentifier,
} from './orderByIdentifier';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

assertEqual(
  buildResultSetOrderByText({
    header: { name: 'UserName', columnName: 'UserName' } as any,
    order: 'desc',
    databaseType: 'POSTGRESQL' as any,
  }),
  '"UserName" desc',
  'PostgreSQL order-by preserves uppercase column names with double quotes',
);

assertEqual(
  buildResultSetOrderByText({
    header: { name: 'select', columnName: 'select' } as any,
    order: 'asc',
    databaseType: 'POSTGRESQL' as any,
  }),
  '"select" asc',
  'PostgreSQL order-by quotes reserved-word-looking column names',
);

assertEqual(
  buildResultSetOrderByText({
    header: { name: 'DisplayName', columnName: 'raw_column' } as any,
    order: 'asc',
    databaseType: 'POSTGRESQL' as any,
  }),
  '"raw_column" asc',
  'PostgreSQL order-by prefers JDBC source columnName over display label',
);

assertEqual(
  buildResultSetOrderByText({
    header: { name: 'UserName', columnName: 'UserName' } as any,
    order: 'desc',
    databaseType: 'MYSQL' as any,
  }),
  'UserName desc',
  'non PostgreSQL-family order-by keeps the existing unquoted behavior',
);

assertEqual(
  quoteDoubleQuotedIdentifier('"alreadyQuoted"'),
  '"alreadyQuoted"',
  'already double-quoted identifiers are not quoted again',
);

assertEqual(
  quoteDoubleQuotedIdentifier('weird"name'),
  '"weird""name"',
  'embedded double quotes are escaped inside quoted identifiers',
);

console.log('ResultSetTable orderByIdentifier tests passed');
