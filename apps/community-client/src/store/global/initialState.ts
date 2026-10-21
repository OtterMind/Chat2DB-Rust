import { CommonState, initialCommonState } from './slices/common/initialState';
import { GlobalSettingState, initialSettingState } from './slices/settings/initialState';
import { RequestState, initialRequestState } from './slices/request/initialState';
import { MiscState, initialMiscState } from './slices/misc/initialState';
import { HotUpdateState, initialHotUpdateState } from './slices/hotUpdate/initialState';

export type GlobalState = CommonState & GlobalSettingState & RequestState & MiscState & HotUpdateState;

export const initialState: GlobalState = {
  ...initialCommonState,
  ...initialSettingState,
  ...initialRequestState,
  ...initialMiscState,
  ...initialHotUpdateState,
};
