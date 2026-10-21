import { PanelState, initPanelState } from './slices/panel/initialState';
import { DataCollectionState, initDataCollectionState } from './slices/dataCollection/initialState';
import { ModelState, initModelState } from './slices/model/initialState';

export type AIState = PanelState & DataCollectionState & ModelState;

export const initialState: AIState = {
  ...initPanelState,
  ...initDataCollectionState,
  ...initModelState,
};
