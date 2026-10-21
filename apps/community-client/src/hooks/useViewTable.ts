import { useCallback, useState } from 'react';
import { IManageResultData, IViewTableParams } from '@/typings';
import executeSqlServer from '@/service/executeSql';
import useAbortRequest from './useAbortRequest';
import { useGlobalStore } from '@/store/global';
import { settingSelectors } from '@/store/global/selectors';

const useViewTable = () => {
  const defaultPageSize = useGlobalStore((state) => settingSelectors.currentBaseSetting(state).defaultPageSize);
  const [executing, setExecuting] = useState(false);
  // interrupt request
  const [initSignal, abortRequest] = useAbortRequest();

  // execute sql
  const executeSQL = useCallback((params: IViewTableParams): Promise<IManageResultData[]> => {

    return new Promise((resolve, reject) => {
      // Parameters for executing sql
      const viewTableParams = {
        ...params,
        pageNo: params.pageNo ?? 1,
        pageSize: params.pageSize ?? defaultPageSize,
      };

      setExecuting(true);

      // View table
      return executeSqlServer
        .viewTable(viewTableParams, {
          signal: initSignal(),
        })
        .then((res) => {
          resolve(res);
        })
        .catch((err) => {
          reject(err);
        })
        .finally(() => { 
          setExecuting(false);
        });
    });
  }, [defaultPageSize])
  
  // Stop executing sql
  const stopExecuteSQL = useCallback(() => {
    abortRequest();
    setExecuting(false);
  },[abortRequest]) 

  return {
    executing,
    executeSQL,
    stopExecuteSQL,
  };
};

export default useViewTable;
