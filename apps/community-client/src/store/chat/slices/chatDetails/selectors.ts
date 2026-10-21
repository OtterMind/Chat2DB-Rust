import { ChatStore } from '@/store/chat';

// Pass in an id and retrieve the chatDetail corresponding to the current id.
export const getChatDetailById = (id: string) => {
  return (state: ChatStore) => {
    const { answers, question } = state.chatDetails?.[id] || {};
    return {
      answers,
      question,
    };
  };
};
