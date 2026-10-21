import { SendParams } from '@/blocks/AI/components/AIChatInput';
import { IPageParams } from '@/typings';
import { ChatVO } from '@/typings/chat';

const defaultPageParam: IPageParams = { pageNo: 1, pageSize: 20, hasNextPage: true };

export interface CommonState {
  /** Currently selected Chat */
  currentChat: Record<'workspace' | 'dashboard' | 'chat', ChatVO | null>;
  /** Chat list */
  chatList: ChatVO[];
  /** Query parameters for Chat list */
  chatListParams: IPageParams;
  /** Chat settings pop-up box */
  openSetting: boolean;
  /** Whether Chat is being built */
  isCreating: boolean;
  /** The parameters that need to be automatically filled in the chat input box include part of SendParams */
  chatInputAutoFillParams?: SendParams;
  /** Send message */
  handleSend?: (params: SendParams) => void;
}

export const initCommonState: CommonState = {
  currentChat: {
    workspace: null,
    dashboard: null,
    chat: null,
  },
  chatList: [],
  chatListParams: defaultPageParam,
  openSetting: false,
  isCreating: false,
  handleSend: undefined,
};
