// TODO:
type ChatItem = any;

export interface ChatItemState {
  chatItems: ChatItem[];
}

export const initialState: ChatItemState = {
  chatItems: [],
};
