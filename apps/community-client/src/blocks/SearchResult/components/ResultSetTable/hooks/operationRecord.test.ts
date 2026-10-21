import { buildUpdateOperation } from './operationRecord';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

const oldMessage = 'ins\n\n\nsd\na\nd\nasda\nsdsa\nd\nasdad\nasdaa';
const changedSql = 'select \n  * \nfrom \n  ai_chat_message;';
const operation = buildUpdateOperation(
  '1',
  {
    CHAT2DB_ROW_NUMBER: '1',
    1: '1409bc33-0143-4aff-b0d8-d513165c7e33',
    6: changedSql,
  },
  [
    {
      field: '6',
      rowId: '1',
      rawValue: oldMessage,
      currentValue: oldMessage,
      changedValue: changedSql,
    },
  ],
);

assertEqual(
  operation.dataList[6],
  changedSql,
  'update operation keeps changed multiline value in dataList',
);

assertEqual(
  operation.oldDataList[6],
  oldMessage,
  'update operation keeps original multiline value in oldDataList even when origin row has changed',
);

console.log('operationRecord tests passed');
