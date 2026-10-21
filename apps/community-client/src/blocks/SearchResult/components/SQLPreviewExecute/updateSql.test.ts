import {
  appendMissingOldDataList,
  buildUpdateSqlRequestParams,
  getRequestErrorMessage,
  resolveUpdateExecuteParams,
} from './updateSql';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

const changedMultilineSql = 'select \n  * \nfrom \n  ai_chat_message;';
const oldMultilineText = 'ins\n\ns\nda\nsd';
const resultData = {
  tableName: 'ai_chat_message',
  headerList: [{ name: 'id' }, { name: 'content' }],
  executeSqlParams: {
    databaseType: 'MYSQL',
    dataSourceId: 46,
    databaseName: 'enterprise_gateway_dev',
    resultSetId: 1,
  },
  dataList: [
    [{ value: 'row-1' }, { value: '42' }, { value: oldMultilineText }],
  ],
};

const operationWithOldDataList = {
  rowId: 'row-1',
  type: 'UPDATE',
  dataList: ['row-1', '42', changedMultilineSql],
  oldDataList: ['row-1', '42', oldMultilineText],
};

assertEqual(
  appendMissingOldDataList([operationWithOldDataList], resultData),
  [operationWithOldDataList],
  'existing oldDataList is preserved and not rebuilt from possibly changed grid data',
);

assertEqual(
  appendMissingOldDataList([{ rowId: 'row-1', type: 'UPDATE', dataList: ['row-1', '42', changedMultilineSql] }], resultData),
  [
    {
      rowId: 'row-1',
      type: 'UPDATE',
      dataList: ['row-1', '42', changedMultilineSql],
      oldDataList: ['row-1', '42', oldMultilineText],
    },
  ],
  'missing oldDataList is filled from the original result row for compatibility',
);

assertEqual(
  buildUpdateSqlRequestParams([operationWithOldDataList], resultData),
  {
    ...resultData.executeSqlParams,
    tableName: 'ai_chat_message',
    headerList: resultData.headerList,
    operations: [operationWithOldDataList],
  },
  'update SQL request keeps table metadata and operation payload together',
);

assertEqual(
  getRequestErrorMessage({ errorMessage: 'SQL syntax error near where' }),
  'SQL syntax error near where',
  'request error message prefers server wrapper errorMessage',
);

assertEqual(
  getRequestErrorMessage('SQL execution failed'),
  'SQL execution failed',
  'request error message keeps string errors',
);

async function main() {
  let getUpdateDataSqlParams: any;
  const resolved = await resolveUpdateExecuteParams({
    operations: [operationWithOldDataList],
    resultData,
    getUpdateDataSql: async (params) => {
      getUpdateDataSqlParams = params;
      return `UPDATE ai_chat_message set \`content\` = '${changedMultilineSql}' where \`id\` = '42' LIMIT 1;`;
    },
  });

  assertEqual(
    getUpdateDataSqlParams.operations[0].dataList[2],
    changedMultilineSql,
    'get_update_sql receives the full multiline changed value',
  );
  assertEqual(
    resolved.sql,
    `UPDATE ai_chat_message set \`content\` = '${changedMultilineSql}' where \`id\` = '42' LIMIT 1;`,
    'resolved execute params keep the full multiline generated SQL',
  );

  console.log('SQLPreviewExecute update SQL tests passed');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
