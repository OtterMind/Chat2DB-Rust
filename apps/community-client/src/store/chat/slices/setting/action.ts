import { ChatVO } from '@/typings/chat';
import { ChatStore } from '../../store';
import { StateCreator } from 'zustand';

export interface SettingAction {
  /** Set Chat VO in Setting */
  setSettingChatVO: (chatVO: ChatVO) => void;
  /** Set the Setting pop-up box */
  setOpenSettingModal: (open: boolean) => void;
  /** Set the currently edited Chat */
  setSettingInfo: (chat: ChatVO | undefined) => void;
}

export const createSettingAction: StateCreator<ChatStore, [['zustand/devtools', never]], [], SettingAction> = (
  set,
  _get,
) => ({
  setSettingChatVO: (chatVO) => {
    set({ settingChatVO: chatVO });
  },
  setOpenSettingModal: (isOpenSetting) => {
    set({ isOpenSetting });
  },
  setSettingInfo: (chat) => {
    set({ settingInfo: chat });
  },
});
