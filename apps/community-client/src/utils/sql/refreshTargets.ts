import { IManageResultData, IRefreshTargets } from '@/typings';
import { DatabaseTypeCode } from '@/constants';
import { SqlTypeEnum } from '@/typings/sqlParser';
import { resultAndTreeNodeMap } from '@/utils/database';
import { useTreeStore } from '@/store/tree';

/** Merge which nodes to refresh */
export const combineRefreshTargets = (sqlResult: IManageResultData[]) => {
  const sqlTypeMap: Record<SqlTypeEnum, IRefreshTargets[]> = sqlResult.reduce((acc, cur) => {
    const { sqlType, refreshTargets, success } = cur;
    if (!success) {
      return acc;
    }

    if (!acc[sqlType]) {
      acc[sqlType] = [];
    }

    acc[sqlType] = acc[sqlType].concat(refreshTargets);
    return acc;
  }, {} as Record<SqlTypeEnum, IRefreshTargets[]>);

  const mergedSqlTypeMap: Record<SqlTypeEnum, IRefreshTargets[]> = Object.entries(sqlTypeMap).reduce(
    (acc, [sqlType, refreshTargetsArray]) => {
      const uniqueRefreshTargets = (refreshTargetsArray || []).reduce(
        (unique: IRefreshTargets[], current: IRefreshTargets) => {
          const isUnique = !unique.some((item: IRefreshTargets) => JSON.stringify(item) === JSON.stringify(current));
          if (isUnique) {
            unique.push(current);
          }
          return unique;
        },
        [],
      );
      acc[sqlType] = uniqueRefreshTargets;
      return acc;
    },
    {} as Record<SqlTypeEnum, IRefreshTargets[]>,
  );

  const flattendResult = Object.entries(mergedSqlTypeMap).flatMap(([sqlType, refreshTargets]) =>
    refreshTargets.map((refreshTarget) => ({ sqlType: sqlType as SqlTypeEnum, ...refreshTarget })),
  );

  return flattendResult;
};

/**
 * Refresh the tree based on the results of executing SQL
 * @param sqlResult
 */
const handleRefreshTreeByExecuteSQL = (sqlResult: IManageResultData[], databaseType: DatabaseTypeCode) => {
  const updateTreeNodeDataByDetail = useTreeStore.getState().updateTreeNodeDataByDetail;

  const refreshTargets = combineRefreshTargets(sqlResult);

  for (const refreshTarget of refreshTargets) {
    const { dataSourceId, databaseName, schemaName, tableName, sqlType } = refreshTarget;

    // TODO: Waiting for this function to be completed
    // if (sqlType === SqlTypeEnum.USE_DATABASE) {
    //   //replace database
    //   setDBInfo?.({
    //     databaseName,
    //   });
    //   continue;
    // }

    // if (sqlType === SqlTypeEnum.SET_SCHEMA) {
    //   //replace schema
    //   setDBInfo?.({
    //     dataSourceId,
    //     databaseName,
    //     schemaName,
    //   });
    //   continue;
    // }

    const treeNodeType = resultAndTreeNodeMap(sqlType, databaseType);
    if (treeNodeType) {
      updateTreeNodeDataByDetail({
        dataSourceId,
        databaseName,
        schemaName,
        tableName,
        databaseType,
        treeNodeType,
      });
    }
  }
};

export default handleRefreshTreeByExecuteSQL;
