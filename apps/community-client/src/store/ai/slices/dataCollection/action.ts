import { AiDataCollectionItem } from '@/typings/aiDataCollection';
import { StateCreator } from 'zustand';
import { AIStore } from '../../store';
import aiDataCollectionService from '@/service/aiDataCollection';
export interface DataCollectionAction {
  getDataCollectionList: () => Promise<void>;
  setDataCollectionList: (dataCollectionList: AiDataCollectionItem[]) => void;
}

export const createDataCollectionAction: StateCreator<
  AIStore,
  [['zustand/devtools', never]],
  [],
  DataCollectionAction
> = (set, _get) => ({
  getDataCollectionList: async () => {
    const dataCollectionList = await aiDataCollectionService.getAiDataCollectionList({
      pageNo: 1,
      pageSize: 1000,
    });
    set({ dataCollectionList: dataCollectionList.data });
  },
  setDataCollectionList: (dataCollectionList: AiDataCollectionItem[]) => {
    set({ dataCollectionList });
  },
});
