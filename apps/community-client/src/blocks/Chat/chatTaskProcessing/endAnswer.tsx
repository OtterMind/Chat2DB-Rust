import { TaskStatus } from '@/constants';
import { useChatStore } from '@/store/chat';
import magicStickServices from '@/service/magicStick';
import { AnswerPartsStatus, AnswerPartsType } from '@/constants/chat';

export const endAnswer = (questionId: number, getRecommendListParams, step) => {
  const { updateAnswerStatus, appendAnswerParts } = useChatStore.getState();
  updateAnswerStatus(questionId, TaskStatus.FINISH);

  appendAnswerParts({
    data: {
      status: AnswerPartsStatus.LOADING,
      partType: AnswerPartsType.RECOMMEND_QUESTION,
      step,
    },
    questionId,
  });

  magicStickServices
    .getRecommendList(getRecommendListParams)
    .then((res: any) => {
      // Do not display recommendations when this is no longer the latest question.
      const chatDetailsIds = useChatStore.getState().chatDetailsIds;
      const lastQuestionId = chatDetailsIds?.[chatDetailsIds.length - 1];
      let isLast = false;
      if (lastQuestionId) {
        isLast = useChatStore.getState().chatDetails?.[lastQuestionId].question?.id === questionId;
      }

      if (!isLast) {
        return;
      }

      appendAnswerParts({
        data: {
          status: AnswerPartsStatus.FINISH,
          partType: AnswerPartsType.RECOMMEND_QUESTION,
          recommends: res,
          step,
        },
        questionId,
      });
    })
    .catch(() => {
      appendAnswerParts({
        data: {
          status: AnswerPartsStatus.FAIL,
          partType: AnswerPartsType.RECOMMEND_QUESTION,
          step,
        },
        questionId,
      });
    });
};
