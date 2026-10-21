import type { IColumnItemNew } from '@/typings';

const DEFAULT_DECIMAL_COLUMN_SIZE = 10;
const DECIMAL_TYPES = new Set(['DECIMAL', 'DECIMAL UNSIGNED']);

function hasDecimalDigits(decimalDigits: IColumnItemNew['decimalDigits']) {
  return decimalDigits !== null && decimalDigits !== undefined && String(decimalDigits).trim() !== '';
}

export function normalizeColumnForSubmit(column: IColumnItemNew): IColumnItemNew {
  const columnType = column.columnType?.toUpperCase();
  if (
    columnType &&
    DECIMAL_TYPES.has(columnType) &&
    column.columnSize == null &&
    hasDecimalDigits(column.decimalDigits)
  ) {
    return {
      ...column,
      columnSize: DEFAULT_DECIMAL_COLUMN_SIZE,
    };
  }
  return column;
}
