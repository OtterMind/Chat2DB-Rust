import { IDashboardItem } from '@/typings';

export interface SettingState {
  /** Set up ChatVO in the pop-up box */
  settingDashboard?: IDashboardItem;
}

export const initSettingState: SettingState = {
  settingDashboard: undefined,
};
