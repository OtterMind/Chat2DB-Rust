import {
  parameterHintContextFromEditorHints,
  parameterHintContextFromInsertValue,
  parameterHintContextFromRoutine,
} from './parameterHint';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

assertEqual(
  parameterHintContextFromInsertValue({
    rowIndex: 0,
    columnIndex: 1,
    fieldName: 'name',
    hints: [
      {
        rowIndex: 0,
        columnIndex: 0,
        fieldName: 'id',
        label: 'id:BIGINT',
        fieldType: 'BIGINT',
        range: { startLineNumber: 1, startColumn: 28, endLineNumber: 1, endColumn: 29 },
        active: false,
      },
      {
        rowIndex: 0,
        columnIndex: 1,
        fieldName: 'name',
        label: 'name:VARCHAR',
        fieldType: 'VARCHAR',
        range: { startLineNumber: 1, startColumn: 31, endLineNumber: 1, endColumn: 32 },
        active: true,
      },
    ],
    highlightRanges: [],
    rowRange: { startLineNumber: 1, startColumn: 27, endLineNumber: 1, endColumn: 33 },
    valueRange: { startLineNumber: 1, startColumn: 31, endLineNumber: 1, endColumn: 32 },
    editingValue: true,
  }),
  {
    source: 'INSERT_VALUE',
    anchorRange: { startLineNumber: 1, startColumn: 31, endLineNumber: 1, endColumn: 32 },
    rowRange: { startLineNumber: 1, startColumn: 27, endLineNumber: 1, endColumn: 33 },
    valueRange: { startLineNumber: 1, startColumn: 31, endLineNumber: 1, endColumn: 32 },
    items: [
      {
        index: 0,
        fieldName: 'id',
        fieldType: 'BIGINT',
        label: 'id:BIGINT',
        range: { startLineNumber: 1, startColumn: 28, endLineNumber: 1, endColumn: 29 },
        active: false,
      },
      {
        index: 1,
        fieldName: 'name',
        fieldType: 'VARCHAR',
        label: 'name:VARCHAR',
        range: { startLineNumber: 1, startColumn: 31, endLineNumber: 1, endColumn: 32 },
        active: true,
      },
    ],
  },
  'insert value hint context adapts to common parameter hint context',
);

assertEqual(
  parameterHintContextFromRoutine({
    anchorRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
    hints: [
      {
        parameterIndex: 0,
        parameterName: 'tenant_id',
        parameterType: 'BIGINT',
        label: 'tenant_id:BIGINT',
        range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
        active: true,
      },
    ],
  }),
  {
    source: 'ROUTINE_PARAMETER',
    anchorRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
    items: [
      {
        index: 0,
        fieldName: 'tenant_id',
        fieldType: 'BIGINT',
        label: 'tenant_id:BIGINT',
        range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
        active: true,
      },
    ],
  },
  'routine parameter hint context adapts to common parameter hint context',
);

assertEqual(
  parameterHintContextFromEditorHints([
    {
      type: 'INSERT_VALUE',
      rowRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 28 },
      valueRange: { startLineNumber: 1, startColumn: 21, endLineNumber: 1, endColumn: 22 },
      items: [
        {
          rowIndex: 0,
          columnIndex: 0,
          fieldName: 'id',
          fieldType: 'BIGINT',
          label: 'id:BIGINT',
          range: { startLineNumber: 1, startColumn: 21, endLineNumber: 1, endColumn: 22 },
          active: true,
        },
      ],
    },
    {
      type: 'ROUTINE_PARAMETER',
      rowRange: { startLineNumber: 1, startColumn: 38, endLineNumber: 1, endColumn: 41 },
      valueRange: { startLineNumber: 1, startColumn: 40, endLineNumber: 1, endColumn: 41 },
      items: [
        {
          rowIndex: 0,
          columnIndex: 0,
          fieldName: 'tenant_id',
          fieldType: 'BIGINT',
          label: 'tenant_id:BIGINT',
          range: { startLineNumber: 1, startColumn: 40, endLineNumber: 1, endColumn: 41 },
          active: true,
        },
      ],
    },
  ]),
  {
    source: 'ROUTINE_PARAMETER',
    anchorRange: { startLineNumber: 1, startColumn: 40, endLineNumber: 1, endColumn: 41 },
    items: [
      {
        index: 0,
        fieldName: 'tenant_id',
        fieldType: 'BIGINT',
        label: 'tenant_id:BIGINT',
        range: { startLineNumber: 1, startColumn: 40, endLineNumber: 1, endColumn: 41 },
        active: true,
      },
    ],
  },
  'backend routine parameter hints take priority over insert value hints',
);

assertEqual(
  parameterHintContextFromEditorHints(
    [
      {
        type: 'ROUTINE_PARAMETER',
        rowRange: { startLineNumber: 1, startColumn: 19, endLineNumber: 1, endColumn: 24 },
        valueRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
        items: [
          {
            rowIndex: 0,
            columnIndex: 0,
            fieldName: 'a',
            fieldType: 'INT',
            label: 'a:INT',
            range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
            active: true,
          },
        ],
      },
    ],
    { lineNumber: 1, column: 12 },
  ),
  null,
  'routine parameter hints are hidden when the cursor is on the routine name',
);

assertEqual(
  parameterHintContextFromEditorHints(
    [
      {
        type: 'ROUTINE_PARAMETER',
        rowRange: { startLineNumber: 1, startColumn: 19, endLineNumber: 1, endColumn: 24 },
        valueRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
        items: [
          {
            rowIndex: 0,
            columnIndex: 0,
            fieldName: 'a',
            fieldType: 'INT',
            label: 'a:INT',
            range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
            active: true,
          },
        ],
      },
    ],
    { lineNumber: 1, column: 20 },
  ),
  {
    source: 'ROUTINE_PARAMETER',
    anchorRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
    items: [
      {
        index: 0,
        fieldName: 'a',
        fieldType: 'INT',
        label: 'a:INT',
        range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
        active: true,
      },
    ],
  },
  'routine parameter hints are shown when the cursor is inside the routine argument list',
);

assertEqual(
  parameterHintContextFromEditorHints([
    {
      type: 'INSERT_VALUE',
      rowRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 28 },
      valueRange: { startLineNumber: 1, startColumn: 24, endLineNumber: 1, endColumn: 25 },
      items: [
        {
          rowIndex: 0,
          columnIndex: 0,
          fieldName: 'id',
          fieldType: 'BIGINT',
          label: 'id:BIGINT',
          range: { startLineNumber: 1, startColumn: 21, endLineNumber: 1, endColumn: 22 },
        },
        {
          rowIndex: 0,
          columnIndex: 1,
          fieldName: 'name',
          fieldType: 'VARCHAR',
          label: 'name:VARCHAR',
          range: { startLineNumber: 1, startColumn: 24, endLineNumber: 1, endColumn: 25 },
          active: true,
        },
      ],
    },
  ]),
  {
    source: 'INSERT_VALUE',
    anchorRange: { startLineNumber: 1, startColumn: 24, endLineNumber: 1, endColumn: 25 },
    rowRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 28 },
    valueRange: { startLineNumber: 1, startColumn: 24, endLineNumber: 1, endColumn: 25 },
    items: [
      {
        index: 0,
        fieldName: 'id',
        fieldType: 'BIGINT',
        label: 'id:BIGINT',
        range: { startLineNumber: 1, startColumn: 21, endLineNumber: 1, endColumn: 22 },
        active: false,
      },
      {
        index: 1,
        fieldName: 'name',
        fieldType: 'VARCHAR',
        label: 'name:VARCHAR',
        range: { startLineNumber: 1, startColumn: 24, endLineNumber: 1, endColumn: 25 },
        active: true,
      },
    ],
  },
  'backend insert value hints adapt to the common parameter hint context',
);

assertEqual(
  parameterHintContextFromEditorHints(
    [
      {
        type: 'ROUTINE_PARAMETER',
        rowRange: { startLineNumber: 1, startColumn: 19, endLineNumber: 1, endColumn: 25 },
        valueRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 24 },
        items: [
          {
            rowIndex: 0,
            columnIndex: 0,
            fieldName: 'a',
            fieldType: 'INT',
            label: 'a:INT',
            range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
            active: true,
          },
          {
            rowIndex: 0,
            columnIndex: 1,
            fieldName: 'b',
            fieldType: 'INT',
            label: 'b:INT',
            range: { startLineNumber: 1, startColumn: 23, endLineNumber: 1, endColumn: 24 },
            active: false,
          },
        ],
      },
    ],
    { lineNumber: 1, column: 22 },
  )?.items.map((item) => item.active),
  [false, true],
  'routine parameter hint active item moves to the next slot when the cursor is between arguments',
);

assertEqual(
  parameterHintContextFromEditorHints(
    [
      {
        type: 'ROUTINE_PARAMETER',
        rowRange: { startLineNumber: 1, startColumn: 19, endLineNumber: 1, endColumn: 25 },
        valueRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 24 },
        items: [
          {
            rowIndex: 0,
            columnIndex: 0,
            fieldName: 'a',
            fieldType: 'INT',
            label: 'a:INT',
            range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
            active: true,
          },
          {
            rowIndex: 0,
            columnIndex: 1,
            fieldName: 'b',
            fieldType: 'INT',
            label: 'b:INT',
            range: { startLineNumber: 1, startColumn: 23, endLineNumber: 1, endColumn: 24 },
            active: false,
          },
        ],
      },
    ],
    { lineNumber: 1, column: 25 },
  )?.items.map((item) => item.active),
  [false, true],
  'routine parameter hint active item stays on the last slot at the closing boundary',
);

console.log('parameterHint tests passed');
