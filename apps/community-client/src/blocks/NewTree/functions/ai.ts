import aiDataCollectionService from '@/service/aiDataCollection';
import { TreeNodeData } from '@/typings';
import i18n from '@/i18n';
import { staticMessage } from '@chat2db/ui';

export const syncAiDataCollection = async (props: { treeNodeData: TreeNodeData }) => {
  const { treeNodeData } = props;
  const { extraParams } = treeNodeData;
  const { aiDataCollectionId, dataSourceId } = extraParams || {};

  const params = {
    id: aiDataCollectionId!,
    dataSourceId: dataSourceId!,
  };

  await aiDataCollectionService.syncDataCollection(params);
  staticMessage.success(i18n('workspace.tips.syncDataBase'));
};
