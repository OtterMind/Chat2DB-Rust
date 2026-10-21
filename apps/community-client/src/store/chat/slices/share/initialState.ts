import { ChatVO } from '@/typings/chat';

export interface ShareState {
  /** Whether the Chat sharing pop-up box is open */
  isOpenShare: boolean;

  /** Share ChatVO in the pop-up box */
  shareChatVO?: ChatVO;
}

export const initShareState: ShareState = {
  isOpenShare: false,
  shareChatVO: undefined,
};
