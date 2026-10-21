import { CommonState, initCommonState } from './slices/common/initialState';
import { SettingState, initSettingState } from './slices/setting/initialState';

export type DashboardState = CommonState & SettingState;

export const initialState: DashboardState = {
  ...initCommonState,
  ...initSettingState,
};
