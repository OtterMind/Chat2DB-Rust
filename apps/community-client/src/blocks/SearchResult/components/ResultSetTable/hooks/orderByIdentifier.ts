import { DatabaseTypeCode } from '@/constants/common';
import { ITableHeaderItem } from '@/typings';

const DOUBLE_QUOTE_IDENTIFIER_DATABASE_TYPES = new Set<DatabaseTypeCode>([
  DatabaseTypeCode.POSTGRESQL,
  DatabaseTypeCode.KINGBASE,
  DatabaseTypeCode.OPENGAUSS,
  DatabaseTypeCode.COCKROACHDB,
  DatabaseTypeCode.GAUSSDB,
]);

const isDoubleQuotedIdentifier = (identifier: string) => /^"(?:[^"]|"")+"$/.test(identifier);

export const quoteDoubleQuotedIdentifier = (identifier: string) => {
  if (isDoubleQuotedIdentifier(identifier)) {
    return identifier;
  }
  return `"${identifier.replace(/"/g, '""')}"`;
};

export const buildResultSetOrderByText = ({
  header,
  order,
  databaseType,
}: {
  header?: ITableHeaderItem | null;
  order: 'asc' | 'desc';
  databaseType?: DatabaseTypeCode;
}) => {
  const columnName = header?.columnName || header?.name;
  if (!columnName) {
    return '';
  }

  const orderByColumn = DOUBLE_QUOTE_IDENTIFIER_DATABASE_TYPES.has(databaseType as DatabaseTypeCode)
    ? quoteDoubleQuotedIdentifier(columnName)
    : columnName;

  return `${orderByColumn} ${order}`;
};
