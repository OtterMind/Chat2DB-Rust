import {
  DEFAULT_VALUE,
  GENERATED_VALUE,
  getBlankCreateCellValue,
  getClonedCreateCellValue,
  normalizeCreateRowPasteValues,
  normalizePasteTargetCell,
  normalizePasteTargetRange,
  transformOperations,
} from './utils';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

const multiLineSql = 'select \n  * \nfrom \n  ai_chat_message;';
const oldMessage = 'ins\n\n\nsd\na\nd\nasda\nsdsa\nd\nasdad\nasdaa';
const headerList = [
  { name: 'Row Number' },
  { name: 'id' },
  { name: 'message' },
] as any;

const transformed = transformOperations(
  [
    {
      rowId: 1,
      type: 'UPDATE',
      dataList: {
        CHAT2DB_ROW_NUMBER: 1,
        1: '42',
        2: multiLineSql,
      },
      oldDataList: {
        CHAT2DB_ROW_NUMBER: 1,
        1: '42',
        2: 'old',
      },
    },
  ],
  headerList,
);

assertEqual(
  transformed[0].dataList,
  ['1', '42', multiLineSql],
  'transformOperations preserves multiline cell values in update payload dataList',
);

assertEqual(
  transformed[0].oldDataList,
  ['1', '42', 'old'],
  'transformOperations keeps row id and oldDataList shape',
);

const transformedChangedOperation = transformOperations(
  [
    {
      rowId: 1,
      type: 'UPDATE',
      dataList: {
        CHAT2DB_ROW_NUMBER: 1,
        1: '42',
        2: multiLineSql,
      },
      oldDataList: {
        CHAT2DB_ROW_NUMBER: 1,
        1: '42',
        2: oldMessage,
      },
    },
  ],
  headerList,
);

assertEqual(
  transformedChangedOperation[0].dataList,
  ['1', '42', multiLineSql],
  'transformOperations keeps changed multiline cell value in dataList',
);

assertEqual(
  transformedChangedOperation[0].oldDataList,
  ['1', '42', oldMessage],
  'transformOperations keeps original multiline cell value in oldDataList',
);

assertEqual(
  getBlankCreateCellValue({ name: 'id', autoIncrement: 1 } as any),
  GENERATED_VALUE,
  'blank create row uses generated sentinel for auto-increment columns',
);

assertEqual(
  getBlankCreateCellValue({ name: 'status', defaultValue: '0' } as any),
  DEFAULT_VALUE,
  'blank create row uses default sentinel for columns with database default',
);

assertEqual(
  getClonedCreateCellValue({ name: 'id', autoIncrement: 1 } as any, '42'),
  GENERATED_VALUE,
  'cloned create row resets auto-increment columns to generated sentinel',
);

assertEqual(
  getClonedCreateCellValue({ name: 'status', defaultValue: '0' } as any, 'enabled'),
  'enabled',
  'cloned create row preserves non-auto-increment copied values even when defaults exist',
);

assertEqual(
  normalizeCreateRowPasteValues({
    values: [['42', 'Alice']],
    startCol: 1,
    startRow: 5,
    columns: [
      { originalData: { name: 'id', autoIncrement: 1 } as any },
      { originalData: { name: 'name' } as any },
    ],
    getRowId: () => 'new-row',
    isCreateRow: (rowId) => rowId === 'new-row',
  }),
  [[GENERATED_VALUE, 'Alice']],
  'paste into a create row resets auto-increment columns to generated sentinel',
);

assertEqual(
  normalizeCreateRowPasteValues({
    values: [['42', 'Alice']],
    startCol: 1,
    startRow: 5,
    columns: [
      { originalData: { name: 'id', autoIncrement: 1 } as any },
      { originalData: { name: 'name' } as any },
    ],
    getRowId: () => 'existing-row',
    isCreateRow: (rowId) => rowId === 'new-row',
  }),
  [['42', 'Alice']],
  'paste into an existing row preserves auto-increment columns',
);

assertEqual(
  normalizePasteTargetCell({ row: 5, col: 0 }),
  { row: 5, col: 1 },
  'paste target cell maps row-number column to first data column',
);

assertEqual(
  normalizePasteTargetCell({ row: 0, col: 3 }),
  { row: 1, col: 3 },
  'paste target cell maps header row to first data row',
);

assertEqual(
  normalizePasteTargetRange({
    startCol: 0,
    startRow: 5,
    endCol: 0,
    endRow: 5,
  }),
  { col: 1, row: 5, maxCol: 1, maxRow: 5 },
  'paste target range maps a row-number-only selection to first data column',
);

assertEqual(
  normalizePasteTargetRange({
    startCol: 3,
    startRow: 2,
    endCol: 0,
    endRow: 0,
  }),
  { col: 1, row: 1, maxCol: 3, maxRow: 2 },
  'paste target range normalizes reversed selections into the data area',
);

console.log('SearchResult utils tests passed');
