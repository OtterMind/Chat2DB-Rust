import {
  getInsertValueHighlightRanges,
  getInsertValueHintContext,
  getInsertValueMismatchMarkers,
  insertValueHintContextFromEditorHint,
  shouldAutoShowInsertValueHint,
  TextRange,
} from './insertValueHighlight';
import {
  InsertValueMappingStatusEnum,
  SimpleInsertValueMapping,
  SqlStatement,
  SqlTypeEnum,
  StatementValidTypeEnum,
} from '@/typings/sqlParser';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function createInsertStatement(insertValueMappings: SimpleInsertValueMapping[]): SqlStatement {
  return {
    sql: 'insert into user_info (id, name, age) values (1, "Tom", 18), (2, "Jerry", 20);',
    sqlStartRowNum: 1,
    sqlStartColNum: 1,
    sqlEndRowNum: 1,
    sqlEndColNum: 84,
    type: SqlTypeEnum.INSERT,
    statementType: StatementValidTypeEnum.VALID,
    comment: '',
    identifiers: [],
    tableColumns: [
      {
        simpleTable: {
          datasourceName: 'local',
          databaseName: 'demo',
          schemaName: '',
          tableName: 'user_info',
          tableAlias: '',
          comment: '',
          insertText: 'user_info',
        },
        simpleColumns: [
          {
            datasourceName: 'local',
            databaseName: 'demo',
            schemaName: '',
            tableName: 'user_info',
            columnName: 'id',
            dataType: 'bigint',
            comment: '',
            insertText: 'id',
          },
          {
            datasourceName: 'local',
            databaseName: 'demo',
            schemaName: '',
            tableName: 'user_info',
            columnName: 'name',
            dataType: 'varchar',
            comment: '',
            insertText: 'name',
          },
        ],
      },
    ],
    insertValueMappings,
  };
}

function mapping(
  column: TextRange,
  value: TextRange,
  rowIndex: number,
  columnIndex: number,
  row: TextRange = rowIndex === 0 ? firstRowRange : secondRowRange,
): SimpleInsertValueMapping {
  return {
    columnStartRowNum: column.startLineNumber,
    columnStartColNum: column.startColumn,
    columnEndRowNum: column.endLineNumber,
    columnEndColNum: column.endColumn,
    valueStartRowNum: value.startLineNumber,
    valueStartColNum: value.startColumn,
    valueEndRowNum: value.endLineNumber,
    valueEndColNum: value.endColumn,
    rowStartRowNum: row.startLineNumber,
    rowStartColNum: row.startColumn,
    rowEndRowNum: row.endLineNumber,
    rowEndColNum: row.endColumn,
    rowIndex,
    columnIndex,
    mappingStatus: InsertValueMappingStatusEnum.MATCHED,
  };
}

const idColumn = { startLineNumber: 1, startColumn: 24, endLineNumber: 1, endColumn: 26 };
const nameColumn = { startLineNumber: 1, startColumn: 28, endLineNumber: 1, endColumn: 32 };
const ageColumn = { startLineNumber: 1, startColumn: 34, endLineNumber: 1, endColumn: 37 };
const firstIdValue = { startLineNumber: 1, startColumn: 47, endLineNumber: 1, endColumn: 48 };
const firstNameValue = { startLineNumber: 1, startColumn: 50, endLineNumber: 1, endColumn: 55 };
const firstAgeValue = { startLineNumber: 1, startColumn: 57, endLineNumber: 1, endColumn: 59 };
const secondIdValue = { startLineNumber: 1, startColumn: 63, endLineNumber: 1, endColumn: 64 };
const secondNameValue = { startLineNumber: 1, startColumn: 66, endLineNumber: 1, endColumn: 73 };
const secondAgeValue = { startLineNumber: 1, startColumn: 75, endLineNumber: 1, endColumn: 77 };
const firstRowRange = { startLineNumber: 1, startColumn: 46, endLineNumber: 1, endColumn: 60 };
const secondRowRange = { startLineNumber: 1, startColumn: 62, endLineNumber: 1, endColumn: 78 };

const statement = createInsertStatement([
  mapping(idColumn, firstIdValue, 0, 0),
  mapping(nameColumn, firstNameValue, 0, 1),
  mapping(ageColumn, firstAgeValue, 0, 2),
  mapping(idColumn, secondIdValue, 1, 0),
  mapping(nameColumn, secondNameValue, 1, 1),
  mapping(ageColumn, secondAgeValue, 1, 2),
]);
const mismatchMessage = (expectedCount: number, actualCount: number) =>
  `INSERT value count mismatch: expected ${expectedCount}, got ${actualCount}`;

assertEqual(
  getInsertValueHighlightRanges(statement, nameColumn),
  [firstNameValue, secondNameValue],
  'highlight backend-mapped values when selecting an insert column',
);

assertEqual(
  getInsertValueHighlightRanges(statement, {
    ...idColumn,
    endColumn: idColumn.startColumn,
  }),
  [firstIdValue, secondIdValue],
  'highlight backend-mapped values when cursor is on a column boundary',
);

assertEqual(
  getInsertValueHighlightRanges(statement, {
    ...secondNameValue,
    endColumn: secondNameValue.startColumn + 3,
  }),
  [nameColumn],
  'highlight backend-mapped column when selecting a value',
);

assertEqual(
  getInsertValueHighlightRanges(statement, {
    startLineNumber: 1,
    startColumn: 8,
    endLineNumber: 1,
    endColumn: 12,
  }),
  [],
  'ignore positions outside backend mappings',
);

assertEqual(
  getInsertValueHighlightRanges({ ...statement, insertValueMappings: undefined }, nameColumn),
  [],
  'do not parse SQL in frontend when backend mappings are missing',
);

assertEqual(
  getInsertValueHighlightRanges({ ...statement, type: SqlTypeEnum.SELECT }, nameColumn),
  [],
  'ignore non-insert statements',
);

assertEqual(
  getInsertValueHintContext(statement, { lineNumber: 1, column: 52 }, (range) =>
    statement.sql.slice(range.startColumn - 1, range.endColumn - 1),
  ),
  {
    rowIndex: 0,
    columnIndex: 1,
    fieldName: 'name',
    fieldType: 'varchar',
    hints: [
      {
        rowIndex: 0,
        columnIndex: 0,
        fieldName: 'id',
        fieldType: 'bigint',
        label: 'id:bigint',
        range: idColumn,
        active: false,
      },
      {
        rowIndex: 0,
        columnIndex: 1,
        fieldName: 'name',
        fieldType: 'varchar',
        label: 'name:varchar',
        range: nameColumn,
        active: true,
      },
      {
        rowIndex: 0,
        columnIndex: 2,
        fieldName: 'age',
        label: 'age',
        range: ageColumn,
        active: false,
      },
    ],
    highlightRanges: [nameColumn],
    rowRange: firstRowRange,
    valueRange: firstNameValue,
    editingValue: true,
  },
  'show insert value hints for an existing value using backend mappings and table column types',
);

const completedValueHintContext = getInsertValueHintContext(statement, { lineNumber: 1, column: 55 }, (range) =>
  statement.sql.slice(range.startColumn - 1, range.endColumn - 1),
);
assertEqual(
  completedValueHintContext?.editingValue,
  false,
  'treat a completed insert value at the value boundary as not actively editing',
);
assertEqual(
  shouldAutoShowInsertValueHint(completedValueHintContext),
  false,
  'do not auto-show the insert value floating hint after the current value is complete',
);

assertEqual(
  getInsertValueHintContext({ ...statement, insertValueMappings: undefined }, { lineNumber: 1, column: 52 }),
  null,
  'do not show insert value hints when backend mappings are missing',
);

const emptyValuesRowStatement = createInsertStatement([
  {
    ...mapping(idColumn, firstIdValue, 0, 0, firstRowRange),
    valueStartRowNum: undefined,
    valueStartColNum: undefined,
    valueEndRowNum: undefined,
    valueEndColNum: undefined,
    mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_COLUMN,
  },
  {
    ...mapping(nameColumn, firstNameValue, 0, 1, firstRowRange),
    valueStartRowNum: undefined,
    valueStartColNum: undefined,
    valueEndRowNum: undefined,
    valueEndColNum: undefined,
    mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_COLUMN,
  },
]);

assertEqual(
  getInsertValueHintContext(emptyValuesRowStatement, { lineNumber: 1, column: 47 }, (range) =>
    emptyValuesRowStatement.sql.slice(range.startColumn - 1, range.endColumn - 1),
  ),
  {
    rowIndex: 0,
    columnIndex: 0,
    fieldName: 'id',
    fieldType: 'bigint',
    hints: [
      {
        rowIndex: 0,
        columnIndex: 0,
        fieldName: 'id',
        fieldType: 'bigint',
        label: 'id:bigint',
        range: idColumn,
        active: true,
      },
      {
        rowIndex: 0,
        columnIndex: 1,
        fieldName: 'name',
        fieldType: 'varchar',
        label: 'name:varchar',
        range: nameColumn,
        active: false,
      },
    ],
    highlightRanges: [idColumn],
    rowRange: firstRowRange,
    editingValue: true,
  },
  'show insert value hints for an empty values row when backend row range is present',
);

const unmappedColumnMapping: SimpleInsertValueMapping = {
  ...mapping(ageColumn, firstAgeValue, 0, 2),
  valueStartRowNum: undefined,
  valueStartColNum: undefined,
  valueEndRowNum: undefined,
  valueEndColNum: undefined,
  mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_COLUMN,
};

const unmappedValueMapping: SimpleInsertValueMapping = {
  ...mapping(ageColumn, secondAgeValue, 1, 3),
  columnStartRowNum: undefined,
  columnStartColNum: undefined,
  columnEndRowNum: undefined,
  columnEndColNum: undefined,
  mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_VALUE,
};

const mismatchStatement = createInsertStatement([
  ...statement.insertValueMappings!,
  unmappedColumnMapping,
  unmappedValueMapping,
]);

assertEqual(
  getInsertValueHighlightRanges(mismatchStatement, ageColumn),
  [firstAgeValue, secondAgeValue],
  'ignore unmapped insert value mappings when highlighting matched columns',
);

assertEqual(
  getInsertValueHighlightRanges(mismatchStatement, secondAgeValue),
  [ageColumn],
  'ignore unmapped insert value mappings when highlighting matched values',
);

assertEqual(
  getInsertValueMismatchMarkers([mismatchStatement]),
  [
    {
      startLineNum: ageColumn.startLineNumber,
      startColNum: ageColumn.startColumn,
      endLineNum: ageColumn.endLineNumber,
      endColNum: ageColumn.endColumn,
      message: mismatchMessage(4, 3),
      type: 'warning',
    },
    {
      startLineNum: secondAgeValue.startLineNumber,
      startColNum: secondAgeValue.startColumn,
      endLineNum: secondAgeValue.endLineNumber,
      endColNum: secondAgeValue.endColumn,
      message: mismatchMessage(3, 4),
      type: 'warning',
    },
  ],
  'create warning markers for unmapped insert columns and values',
);

const extraFirstValue = { startLineNumber: 3, startColumn: 33, endLineNumber: 3, endColumn: 34 };
const extraSecondValue = { startLineNumber: 3, startColumn: 35, endLineNumber: 3, endColumn: 36 };
const extraValueStatement = createInsertStatement([
  mapping(idColumn, firstIdValue, 0, 0),
  mapping(nameColumn, firstNameValue, 0, 1),
  mapping(ageColumn, firstAgeValue, 0, 2),
  {
    ...mapping(ageColumn, extraFirstValue, 0, 3),
    columnStartRowNum: undefined,
    columnStartColNum: undefined,
    columnEndRowNum: undefined,
    columnEndColNum: undefined,
    mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_VALUE,
  },
  {
    ...mapping(ageColumn, extraSecondValue, 0, 4),
    columnStartRowNum: undefined,
    columnStartColNum: undefined,
    columnEndRowNum: undefined,
    columnEndColNum: undefined,
    mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_VALUE,
  },
]);

assertEqual(
  getInsertValueMismatchMarkers([extraValueStatement]),
  [
    {
      startLineNum: extraFirstValue.startLineNumber,
      startColNum: extraFirstValue.startColumn,
      endLineNum: extraSecondValue.endLineNumber,
      endColNum: extraSecondValue.endColumn,
      message: mismatchMessage(3, 5),
      type: 'warning',
    },
  ],
  'merge consecutive unmapped insert values into one warning marker',
);

const missingNameColumn: SimpleInsertValueMapping = {
  ...mapping(nameColumn, firstNameValue, 0, 1),
  valueStartRowNum: undefined,
  valueStartColNum: undefined,
  valueEndRowNum: undefined,
  valueEndColNum: undefined,
  mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_COLUMN,
};
const missingAgeColumn: SimpleInsertValueMapping = {
  ...mapping(ageColumn, firstAgeValue, 0, 2),
  valueStartRowNum: undefined,
  valueStartColNum: undefined,
  valueEndRowNum: undefined,
  valueEndColNum: undefined,
  mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_COLUMN,
};
const missingColumnStatement = createInsertStatement([
  mapping(idColumn, firstIdValue, 0, 0),
  missingNameColumn,
  missingAgeColumn,
]);

assertEqual(
  getInsertValueMismatchMarkers([missingColumnStatement]),
  [
    {
      startLineNum: nameColumn.startLineNumber,
      startColNum: nameColumn.startColumn,
      endLineNum: ageColumn.endLineNumber,
      endColNum: ageColumn.endColumn,
      message: mismatchMessage(3, 1),
      type: 'warning',
    },
  ],
  'merge consecutive unmapped insert columns into one warning marker',
);

const missingAllValueStatement = createInsertStatement([
  {
    ...mapping(idColumn, firstIdValue, 0, 0),
    valueStartRowNum: undefined,
    valueStartColNum: undefined,
    valueEndRowNum: undefined,
    valueEndColNum: undefined,
    mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_COLUMN,
  },
  {
    ...mapping(nameColumn, firstNameValue, 0, 1),
    valueStartRowNum: undefined,
    valueStartColNum: undefined,
    valueEndRowNum: undefined,
    valueEndColNum: undefined,
    mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_COLUMN,
  },
]);

assertEqual(
  getInsertValueMismatchMarkers([missingAllValueStatement]),
  [
    {
      startLineNum: idColumn.startLineNumber,
      startColNum: idColumn.startColumn,
      endLineNum: nameColumn.endLineNumber,
      endColNum: nameColumn.endColumn,
      message: mismatchMessage(2, 0),
      type: 'warning',
    },
  ],
  'show counted warning message when actual insert value count is zero',
);

assertEqual(
  getInsertValueMismatchMarkers([
    createInsertStatement([
      {
        rowIndex: 0,
        columnIndex: 0,
        mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_COLUMN,
      },
      {
        rowIndex: 0,
        columnIndex: 1,
        mappingStatus: InsertValueMappingStatusEnum.UNMAPPED_VALUE,
      },
    ]),
  ]),
  [],
  'skip insert mismatch markers when the target range is missing',
);

assertEqual(
  insertValueHintContextFromEditorHint({
    type: 'INSERT_VALUE',
    rowRange: firstRowRange,
    valueRange: firstNameValue,
    items: [
      {
        rowIndex: 0,
        columnIndex: 0,
        fieldName: 'id',
        fieldType: 'bigint',
        label: 'id:bigint',
        range: idColumn,
      },
      {
        rowIndex: 0,
        columnIndex: 1,
        fieldName: 'name',
        fieldType: 'varchar',
        label: 'name:varchar',
        range: nameColumn,
        active: true,
      },
      {
        rowIndex: 0,
        columnIndex: 2,
        fieldName: 'age',
        fieldType: 'int',
        label: 'age:int',
        range: ageColumn,
      },
    ],
  }),
  {
    rowIndex: 0,
    columnIndex: 1,
    fieldName: 'name',
    fieldType: 'varchar',
    hints: [
      {
        rowIndex: 0,
        columnIndex: 0,
        fieldName: 'id',
        fieldType: 'bigint',
        label: 'id:bigint',
        range: idColumn,
        active: false,
      },
      {
        rowIndex: 0,
        columnIndex: 1,
        fieldName: 'name',
        fieldType: 'varchar',
        label: 'name:varchar',
        range: nameColumn,
        active: true,
      },
      {
        rowIndex: 0,
        columnIndex: 2,
        fieldName: 'age',
        fieldType: 'int',
        label: 'age:int',
        range: ageColumn,
        active: false,
      },
    ],
    highlightRanges: [nameColumn],
    rowRange: firstRowRange,
    valueRange: firstNameValue,
    editingValue: true,
  },
  'create insert value floating hint context from backend editorHints',
);

console.log('insertValueHighlight tests passed');
