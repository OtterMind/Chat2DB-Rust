import { AiDataCollectionItem } from '@/typings/aiDataCollection';

export interface DataCollectionState {
  dataCollectionList: AiDataCollectionItem[];
}

export const initDataCollectionState: DataCollectionState = {
  dataCollectionList: [],
};
