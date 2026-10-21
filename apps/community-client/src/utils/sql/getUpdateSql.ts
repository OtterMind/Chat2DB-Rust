import executeSqlServer, { IUpdateDataSql } from '@/service/executeSql';

// Get a unified entrance to update sql
export function getUpdateSql(props: IUpdateDataSql) {
  return new Promise((resolve, reject) => {
    // execute sql
    return executeSqlServer
      .getUpdateDataSql(props)
      .then((res) => {
        resolve(res);
      })
      .catch(reject);
  });
}
