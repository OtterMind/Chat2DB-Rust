import produce from 'immer';
import type { StateCreator } from 'zustand/vanilla';
import { GlobalStore } from '../../store';
import { MiscState } from './initialState';

export interface MiscAction {
  setLoginType: (loginType: MiscState['loginType']) => void;
  updatePayStatus: (payStatus: MiscState['payStatus']) => void;
  setOpenGuideDialog: (openGuideDialog: MiscState['openGuideDialog']) => void;
  setGuideDialogStatus: (guideDialogStatus: MiscState['guideDialogStatus']) => void;
  setConfetti: (triggerConfetti: MiscState['triggerConfetti']) => void;
  setOpenLinenseDialog: (openLinenseDialog: MiscState['openLinenseDialog']) => void;
  setLicenseDialogType: (licenseDialogType: MiscState['licenseDialogType']) => void;
  setDeleteModal: (deleteModal: MiscState['deleteModal']) => void;
  setWorkspaceAiIntroDismissed: (dismissed: MiscState['workspaceAiIntroDismissed']) => void;
}

export const createMiscAction: StateCreator<GlobalStore, [['zustand/devtools', never]], [], MiscAction> = (set) => ({
  setLoginType: (loginType: MiscState['loginType']) => {
    set(
      produce((state: MiscState) => {
        state.loginType = loginType;
      }),
    );
  },
  updatePayStatus: (payStatus: MiscState['payStatus']) => {
    set(
      produce((state: MiscState) => {
        state.payStatus = payStatus;
      }),
    );
  },
  setOpenGuideDialog: (openGuideDialog: MiscState['openGuideDialog']) => {
    set({
      openGuideDialog,
    });
  },
  setGuideDialogStatus: (guideDialogStatus: MiscState['guideDialogStatus']) => {
    set({
      guideDialogStatus,
    });
  },
  setConfetti: (triggerConfetti: MiscState['triggerConfetti']) => {
    set({
      triggerConfetti,
    });
  },
  setOpenLinenseDialog: (openLinenseDialog: MiscState['openLinenseDialog']) => {
    set({
      openLinenseDialog,
    });
  },
  setLicenseDialogType: (licenseDialogType: MiscState['licenseDialogType']) => {
    set({
      licenseDialogType,
    });
  },
  setDeleteModal: (deleteModal: MiscState['deleteModal']) => {
    set({
      deleteModal,
    });
  },
  setWorkspaceAiIntroDismissed: (workspaceAiIntroDismissed) => {
    set({ workspaceAiIntroDismissed });
  },
});
