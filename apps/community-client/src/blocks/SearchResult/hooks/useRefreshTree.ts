import { IManageResultData, IRefreshTargets } from '@/typings';
import { SqlTypeEnum } from '@/typings/sqlParser';
import { DatabaseTypeCode } from '@/constants';
import { resultAndTreeNodeMap } from '@/utils/database';
import { useTreeStore } from '@/store/tree';
import { useCallback } from 'react';

const useRefreshTree = ({ setBoundInfo }: { setBoundInfo: (IBoundInfo: any) => void }) => {
  const { updateTreeNodeDataByDetail } = useTreeStore((state) => ({
    updateTreeNodeDataByDetail: state.updateTreeNodeDataByDetail,
  }));

  /**
   * Refresh the tree according to the result of executing SQL
   * @param sqlResult
   */
  /** merge which nodes are refreshed */
  const combineRefreshTargets = useCallback((sqlResult: IManageResultData[]) => {
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
  },[]) 

  const handleRefreshTreeByExecuteSQL = useCallback(
    (sqlResult: IManageResultData[], databaseType: DatabaseTypeCode) => {
      const refreshTargets = combineRefreshTargets(sqlResult);

      for (const refreshTarget of refreshTargets) {
        const { dataSourceId, databaseName, schemaName, tableName, sqlType } = refreshTarget;

        if (sqlType === SqlTypeEnum.USE_DATABASE) {
          // replace database
          setBoundInfo((prev) => ({
            ...prev,
            dataSourceId,
            databaseName,
            schemaName,
          }));
          continue;
        }

        if (sqlType === SqlTypeEnum.SET_SCHEMA) {
          // replace schema
          setBoundInfo((prev) => ({
            ...prev,
            dataSourceId,
            databaseName,
            schemaName,
          }));
          continue;
        }

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
    },
    [],
  );

  return handleRefreshTreeByExecuteSQL;
};

export default useRefreshTree;
