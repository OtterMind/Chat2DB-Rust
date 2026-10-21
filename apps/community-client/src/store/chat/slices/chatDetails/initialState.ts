import { ChatDetailVO } from '@/typings/chat';

export interface ChatDetailState {
  chatDetailsIds: string[] | null;
  chatDetails?: {
    [key: string]: ChatDetailVO;
  } | null;
}

export const initChatDetailState: ChatDetailState = {
  chatDetailsIds: null,
  chatDetails: null,
};
