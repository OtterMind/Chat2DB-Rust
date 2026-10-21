import { runtimeEditionConfig } from '@/constants/runtimeEdition';
import { UpdatedStatus } from '@/constants/settings';
import jcefApi from '@/jcef';
import { IHotUpdateConfig } from '@/typings/settings';
import { isDesktop } from '@/utils/env';
import produce from 'immer';
import type { StateCreator } from 'zustand/vanilla';
import { GlobalStore } from '../../store';

export interface HotUpdateAction {
  // Update and restart the app
  updateAndRestartApp: () => void;
  // Check for updates
  handleCheckUpdate: () => Promise<boolean>;
  // Update hot update configuration
  updateHotUpdateConfig: (property: keyof IHotUpdateConfig, value: any) => void;
}

export const createHotUpdateAction: StateCreator<GlobalStore, [['zustand/devtools', never]], [], HotUpdateAction> = (
  set,
  get,
) => ({
  updateAndRestartApp: async () => {
    if (!runtimeEditionConfig.autoUpdate) {
      return;
    }
    if (get().updateDetail.status === UpdatedStatus.Updated) {
      get().setUpdateDetail({
        status: UpdatedStatus.Installing,
      });
      await jcefApi.triggerInstallation();
      get().setUpdateDetail({
        status: UpdatedStatus.UpdateFailed,
      });
    }
    await jcefApi?.restartApp();
  },
  handleCheckUpdate: () => {
    if (!isDesktop || !runtimeEditionConfig.autoUpdate) {
      return Promise.resolve(false);
    }
    return jcefApi.appCheckUpdate().then((res) => {
      get().setUpdateDetail({
        status: res.status,
        version: res.version,
      });
      const flag = res.status === UpdatedStatus.Available;
      return flag;
    });
  },
  updateHotUpdateConfig: (property, value) => {
    set({
      hotUpdateConfig: produce(get().hotUpdateConfig, (draft) => {
        draft[property] = value;
      }),
    });
  },
});
