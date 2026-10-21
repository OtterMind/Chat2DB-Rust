export interface UpdateSqlOperation {
  rowId?: string | number;
  type?: string;
  oldDataList?: any[];
  [key: string]: any;
}

export interface UpdateSqlResultData {
  tableName?: string;
  headerList?: any[];
  dataList?: Array<Array<{ value?: any } | null | undefined>>;
  executeSqlParams?: Record<string, any>;
}

export function appendMissingOldDataList(operations: UpdateSqlOperation[], resultData: UpdateSqlResultData) {
  return operations.map((operation) => {
    if (operation.type !== 'UPDATE' || operation.oldDataList) {
      return operation;
    }
    const oldDataList = resultData.dataList?.find((data) => data[0]?.value === operation.rowId);
    return {
      ...operation,
      oldDataList: oldDataList?.map((cell) => cell?.value ?? null),
    };
  });
}

export function buildUpdateSqlRequestParams(operations: UpdateSqlOperation[], resultData: UpdateSqlResultData) {
  return {
    ...(resultData.executeSqlParams || {}),
    tableName: resultData.tableName,
    headerList: resultData.headerList,
    operations: appendMissingOldDataList(operations, resultData),
  };
}

export async function resolveUpdateExecuteParams(params: {
  operations: UpdateSqlOperation[];
  resultData: UpdateSqlResultData;
  getUpdateDataSql: (requestParams: any) => Promise<string>;
}) {
  const sql = await params.getUpdateDataSql(buildUpdateSqlRequestParams(params.operations, params.resultData));
  return {
    ...(params.resultData.executeSqlParams || {}),
    sql,
  };
}

export function getRequestErrorMessage(error: any) {
  if (typeof error === 'string') {
    return error;
  }
  return error?.errorMessage || error?.message || '';
}
