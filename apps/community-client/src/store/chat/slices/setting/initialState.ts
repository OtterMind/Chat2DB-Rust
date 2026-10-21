import { ChatVO } from '@/typings/chat';

export interface SettingState {
  /** Set up ChatVO in the pop-up box */
  settingChatVO?: ChatVO;
  /** Chat sets whether the pop-up box is open */
  isOpenSetting: boolean;
  // settingInfo
  settingInfo?: ChatVO;
}

export const initSettingState: SettingState = {
  isOpenSetting: false,
  settingChatVO: undefined,
  settingInfo: undefined,
};
