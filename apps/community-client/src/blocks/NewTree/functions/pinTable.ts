// Pinned form
import mysqlService from '@/service/sql';
export const handelPinTable = ({ treeNodeData }) => {
  const api = treeNodeData.decorativeParams.pinned ? 'deleteTablePin' : 'addTablePin';
  return mysqlService[api]({
    dataSourceId: treeNodeData.extraParams.dataSourceId,
    databaseName: treeNodeData.extraParams.databaseName,
    schemaName: treeNodeData.extraParams.schemaName,
    tableName: treeNodeData.originalTitle,
  })
};
