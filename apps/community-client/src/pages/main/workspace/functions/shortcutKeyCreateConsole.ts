import { useWorkspaceStore } from '@/store/workspace';
import { useTreeStore } from '@/store/tree';

export const handelCreateConsole = () => {
  const currentTreeNode = useTreeStore.getState().currentTreeNode;
  const dataSourceList = useTreeStore.getState().dataSourceList;
  const createConsole = useWorkspaceStore.getState().createConsole;

  if (currentTreeNode?.extraParams?.dataSourceId) {
    const param = {
      dataSourceId: currentTreeNode.extraParams.dataSourceId,
      dataSourceName: currentTreeNode.extraParams.dataSourceName!,
      databaseType: currentTreeNode.extraParams.databaseType!,
      databaseName: currentTreeNode.extraParams.databaseName,
      schemaName: currentTreeNode.extraParams.schemaName,
    };
    createConsole(param);
  } else if (dataSourceList?.[0]?.extraParams) {
    const param: any = {
      dataSourceId: dataSourceList[0].extraParams.dataSourceId,
      dataSourceName: dataSourceList[0].extraParams.dataSourceName,
      databaseType: dataSourceList[0].extraParams.databaseType,
    };
    createConsole(param);
  }
};
