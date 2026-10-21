import { QuestionType } from '@/constants/chat';
import { StateCreator } from 'zustand';
import { ChatStore } from '../../store';

export interface AIAction {
  setAIType: (aiType: QuestionType) => void;
}

export const createAIAction: StateCreator<ChatStore, [['zustand/devtools', never]], [], AIAction> = (set, _get) => ({
  setAIType: (aiType) => {
    set({ currentAIType: aiType });
  },
});
