import { IDatabaseBaseInfo, IManageResultData } from '@/typings/database';
import executeSqlServer from '@/service/executeSql';
import { IExecuteSqlParams } from '@/typings';
import { useGlobalStore } from '@/store/global';

export interface IViewTableProps {
  // Open table name
  tableName?: string;
  // Pagination information
  pageSize?: number;
  pageNo?: number;
  // Database information for executing sql
  databaseBaseInfo: IDatabaseBaseInfo;
  // Other information
  // Approval ID
  applyId?: number;
}

// Default paging parameters
// export const defaultPaging = {
//   pageNo: 1,
//   pageSize: 200,
// }

// Unified sql execution entrance unifiedSqlExecutor
export function unifiedSqlExecutor(props: IExecuteSqlParams, signal: any): Promise<IManageResultData[]> {
  const { sql, single, pageSize, pageNo, databaseBaseInfo } = props;
  const errorContinue = useGlobalStore.getState().editorSettings.errorContinue;
  return new Promise((resolve, reject) => {
    // //Interceptor
    // const interceptorRes = interceptor?.(sql);
    // if (interceptorRes) {
    //   return resolve(interceptorRes);
    // }
    // execute sql
    return executeSqlServer
      .executeSql(
        {
          sql,
          single,
          pageSize,
          pageNo,
          errorContinue,
          ...databaseBaseInfo,
        },
        { signal },
      )
      .then((res) => {
        resolve(res);
      })
      .catch(reject);
  });
}

// View table unified entrance
export const unifiedViewTable = (props: IViewTableProps) => {
  const { tableName, pageSize, pageNo, databaseBaseInfo } = props;
  return new Promise((resolve, reject) => {
    // execute sql
    return executeSqlServer
      .viewTable({
        tableName,
        pageSize,
        pageNo,
        ...databaseBaseInfo,
      })
      .then((res) => {
        resolve(res);
      })
      .catch(reject);
  });
};
