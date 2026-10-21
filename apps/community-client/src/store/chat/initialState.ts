import { AIState, initAIState } from './slices/ai/initialState';
import { CommonState, initCommonState } from './slices/common/initialState';
import { SettingState, initSettingState } from './slices/setting/initialState';
import { ShareState, initShareState } from './slices/share/initialState';
import { ChatDetailState, initChatDetailState } from './slices/chatDetails/initialState';

export type ChatState = CommonState & SettingState & ShareState & AIState & ChatDetailState;

export const initialState: ChatState = {
  ...initCommonState,
  ...initSettingState,
  ...initShareState,
  ...initAIState,
  ...initChatDetailState,
};
