import {
  getRoutineParameterHintContext,
  parseRoutineParameterDetail,
  routineParameterHintContextFromEditorHint,
} from './routineParameterHint';
import { TIP_TYPE } from '../type';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

assertEqual(
  parseRoutineParameterDetail('(a:INT, b:DECIMAL(10,2), force:TINYINT IN)'),
  [
    { parameterName: 'a', parameterType: 'INT' },
    { parameterName: 'b', parameterType: 'DECIMAL(10,2)' },
    { parameterName: 'force', parameterType: 'TINYINT IN' },
  ],
  'routine parameter detail parser keeps complex type signatures',
);

assertEqual(
  routineParameterHintContextFromEditorHint({
    type: 'ROUTINE_PARAMETER',
    valueRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
    items: [
      {
        rowIndex: 0,
        columnIndex: 0,
        fieldName: 'left_value',
        fieldType: 'INT',
        label: 'left_value:INT',
        range: { startLineNumber: 1, startColumn: 13, endLineNumber: 1, endColumn: 14 },
      },
      {
        rowIndex: 0,
        columnIndex: 1,
        fieldName: 'right_value',
        fieldType: 'INT',
        label: 'right_value:INT',
        range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
        active: true,
      },
    ],
  }),
  {
    hints: [
      {
        parameterIndex: 0,
        parameterName: 'left_value',
        parameterType: 'INT',
        label: 'left_value:INT',
        range: { startLineNumber: 1, startColumn: 13, endLineNumber: 1, endColumn: 14 },
        active: false,
      },
      {
        parameterIndex: 1,
        parameterName: 'right_value',
        parameterType: 'INT',
        label: 'right_value:INT',
        range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
        active: true,
      },
    ],
    anchorRange: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
  },
  'routine parameter hint context can come from backend editorHints',
);

assertEqual(
  getRoutineParameterHintContext(
    {
      routineName: 'add_numbers',
      routineType: TIP_TYPE.FUNCTION,
      detail: '(left_value:INT, right_value:INT)',
      insertText: 'add_numbers(${1:left_value}, ${2:right_value})',
    },
    { lineNumber: 1, column: 23 },
    () => 'select add_numbers(1, 2)',
  ),
  {
    routineName: 'add_numbers',
    routineType: TIP_TYPE.FUNCTION,
    anchorRange: { startLineNumber: 1, startColumn: 23, endLineNumber: 1, endColumn: 24 },
    hints: [
      {
        parameterIndex: 0,
        parameterName: 'left_value',
        parameterType: 'INT',
        label: 'left_value:INT',
        range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
        active: false,
      },
      {
        parameterIndex: 1,
        parameterName: 'right_value',
        parameterType: 'INT',
        label: 'right_value:INT',
        range: { startLineNumber: 1, startColumn: 23, endLineNumber: 1, endColumn: 24 },
        active: true,
      },
    ],
  },
  'accepted routine snippet tracks the active argument',
);

console.log('routineParameterHint tests passed');
