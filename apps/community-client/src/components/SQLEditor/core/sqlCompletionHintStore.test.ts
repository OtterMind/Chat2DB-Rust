import { createSqlCompletionHintStore, sqlCompletionHintScopeFromRange } from './sqlCompletionHintStore';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

const firstStatementRange = { startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 26 };
const secondStatementRange = { startLineNumber: 3, startColumn: 1, endLineNumber: 3, endColumn: 22 };

const firstRoutineHint = {
  type: 'ROUTINE_PARAMETER',
  statementRange: firstStatementRange,
  rowRange: { startLineNumber: 1, startColumn: 19, endLineNumber: 1, endColumn: 25 },
  items: [
    {
      rowIndex: 0,
      columnIndex: 0,
      fieldName: 'a',
      range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
    },
  ],
};

const secondRoutineHint = {
  type: 'ROUTINE_PARAMETER',
  statementRange: secondStatementRange,
  rowRange: { startLineNumber: 3, startColumn: 13, endLineNumber: 3, endColumn: 21 },
  items: [
    {
      rowIndex: 0,
      columnIndex: 0,
      fieldName: 'value',
      range: { startLineNumber: 3, startColumn: 14, endLineNumber: 3, endColumn: 15 },
    },
  ],
};

const store = createSqlCompletionHintStore();

assertEqual(
  store.commitHints([firstRoutineHint, secondRoutineHint]),
  [firstRoutineHint, secondRoutineHint],
  'non-empty commits add scoped hint buckets',
);

assertEqual(
  store.commitScoped(sqlCompletionHintScopeFromRange('content', firstStatementRange), []),
  [secondRoutineHint],
  'empty scoped commit removes only intersecting statement hints',
);

assertEqual(
  store.commitScoped(sqlCompletionHintScopeFromRange('content', firstStatementRange), [firstRoutineHint]),
  [secondRoutineHint, firstRoutineHint],
  'scoped non-empty commit replaces that statement and keeps unrelated hints',
);

assertEqual(
  store.commitHints([]),
  [],
  'empty unscoped commit replaces all existing scoped hints',
);

assertEqual(store.clear(), [], 'clear removes all hints');

const firstRoutineInSameStatement = {
  ...firstRoutineHint,
  rowRange: { startLineNumber: 1, startColumn: 8, endLineNumber: 1, endColumn: 23 },
  items: [
    {
      rowIndex: 0,
      columnIndex: 0,
      fieldName: 'left_value',
      range: { startLineNumber: 1, startColumn: 19, endLineNumber: 1, endColumn: 20 },
    },
  ],
};

const secondRoutineInSameStatement = {
  ...firstRoutineHint,
  rowRange: { startLineNumber: 1, startColumn: 26, endLineNumber: 1, endColumn: 42 },
  items: [
    {
      rowIndex: 0,
      columnIndex: 0,
      fieldName: 'right_value',
      range: { startLineNumber: 1, startColumn: 38, endLineNumber: 1, endColumn: 39 },
    },
  ],
};

const updatedFirstRoutineInSameStatement = {
  ...firstRoutineInSameStatement,
  items: [
    {
      rowIndex: 0,
      columnIndex: 0,
      fieldName: 'updated_left_value',
      range: { startLineNumber: 1, startColumn: 19, endLineNumber: 1, endColumn: 20 },
    },
  ],
};

const updatedFirstRoutineWithEmptyItems = {
  ...firstRoutineInSameStatement,
  items: [],
};

const sameStatementStore = createSqlCompletionHintStore();
sameStatementStore.commitHints([firstRoutineInSameStatement, secondRoutineInSameStatement]);

assertEqual(
  sameStatementStore.commitScoped(sqlCompletionHintScopeFromRange('content', firstStatementRange), [
    updatedFirstRoutineInSameStatement,
  ]),
  [secondRoutineInSameStatement, updatedFirstRoutineInSameStatement],
  'non-empty scoped commit updates returned hint ranges without deleting other hints in the same statement',
);

const staleItemStore = createSqlCompletionHintStore();
staleItemStore.commitHints([firstRoutineInSameStatement, secondRoutineInSameStatement]);

assertEqual(
  staleItemStore.commitScoped(sqlCompletionHintScopeFromRange('content', firstRoutineInSameStatement.items[0].range), [
    updatedFirstRoutineWithEmptyItems,
  ]),
  [secondRoutineInSameStatement, updatedFirstRoutineWithEmptyItems],
  'non-empty scoped commit removes stale hint in edited slot even when backend omits old item range',
);

const pointScopeStore = createSqlCompletionHintStore();
pointScopeStore.commitHints([firstRoutineInSameStatement, secondRoutineInSameStatement]);

assertEqual(
  pointScopeStore.commitScoped(
    sqlCompletionHintScopeFromRange('content', {
      startLineNumber: 1,
      startColumn: 19,
      endLineNumber: 1,
      endColumn: 19,
    }),
    [],
  ),
  [secondRoutineInSameStatement],
  'empty point-scoped commit removes only the hint touched by the edit position',
);

const boundaryScopeStore = createSqlCompletionHintStore();
boundaryScopeStore.commitHints([firstRoutineInSameStatement, secondRoutineInSameStatement]);

assertEqual(
  boundaryScopeStore.commitScoped(
    sqlCompletionHintScopeFromRange('content', {
      startLineNumber: 1,
      startColumn: 23,
      endLineNumber: 1,
      endColumn: 23,
    }),
    [],
  ),
  [firstRoutineInSameStatement, secondRoutineInSameStatement],
  'point at exclusive end boundary does not remove previous hint',
);

const secondBoundaryScopeStore = createSqlCompletionHintStore();
secondBoundaryScopeStore.commitHints([firstRoutineInSameStatement, secondRoutineInSameStatement]);

assertEqual(
  secondBoundaryScopeStore.commitScoped(
    sqlCompletionHintScopeFromRange('content', {
      startLineNumber: 1,
      startColumn: 26,
      endLineNumber: 1,
      endColumn: 26,
    }),
    [],
  ),
  [firstRoutineInSameStatement],
  'point at next hint start removes that next hint only',
);

const multiItemHintWithoutRowRange = {
  type: 'ROUTINE_PARAMETER',
  statementRange: firstStatementRange,
  items: [
    {
      rowIndex: 0,
      columnIndex: 0,
      fieldName: 'a',
      range: { startLineNumber: 1, startColumn: 10, endLineNumber: 1, endColumn: 11 },
    },
    {
      rowIndex: 0,
      columnIndex: 1,
      fieldName: 'b',
      range: { startLineNumber: 1, startColumn: 20, endLineNumber: 1, endColumn: 21 },
    },
  ],
};

const envelopeScopeStore = createSqlCompletionHintStore();
envelopeScopeStore.commitHints([multiItemHintWithoutRowRange, secondRoutineHint]);

assertEqual(
  envelopeScopeStore.commitScoped(
    sqlCompletionHintScopeFromRange('content', {
      startLineNumber: 1,
      startColumn: 20,
      endLineNumber: 1,
      endColumn: 20,
    }),
    [],
  ),
  [secondRoutineHint],
  'hint without rowRange uses all item ranges as scope envelope',
);

console.log('sqlCompletionHintStore tests passed');
