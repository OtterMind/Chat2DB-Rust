import { StateCreator } from 'zustand';
import { ChatStore } from '../../store';
import { ChatVO } from '@/typings/chat';

export interface ShareAction {
  /** Set up sharing pop-up box */
  setOpenShareModal: (open: boolean) => void;
  /** Share Chat VO in the pop-up box */
  setShareChatVO: (chatVO?: ChatVO) => void;
}

export const createShareAction: StateCreator<ChatStore, [['zustand/devtools', never]], [], ShareAction> = (
  set,
  _get,
) => ({
  setOpenShareModal: (isOpenShare) => {
    set({ isOpenShare });
  },
  setShareChatVO: (chatVO) => {
    set({ shareChatVO: chatVO });
  },
});
