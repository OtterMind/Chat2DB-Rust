import { QuestionType } from '@/constants/chat';

export interface AIState {
  currentAIType: QuestionType;
  // The currently matched table
}

export const initAIState: AIState = {
  currentAIType: QuestionType.NL_2_SQL,
};
