import { StateCreator } from 'zustand';
import { IDashboardItem } from '@/typings';
import { DashboardStore } from '../../store';

export interface SettingAction {
  /** Set the Dashboard in Setting */
  setSettingDashboard: (dashboard?: IDashboardItem) => void;
  /** Handle setting pop-up box confirmation */
  handleConfirmSettingModal: (dashboard: IDashboardItem) => void;
}

export const createSettingAction: StateCreator<DashboardStore, [['zustand/devtools', never]], [], SettingAction> = (
  set,
  get,
) => ({
  setSettingDashboard: (dashboard) => {
    set({ settingDashboard: dashboard });
  },
  handleConfirmSettingModal: async (dashboard) => {
    if (dashboard.id === undefined) {
      await get().createDashboard(dashboard);
    } else {
      await get().updateDashboard(dashboard);
    }
    set({ settingDashboard: undefined });
  },
});
