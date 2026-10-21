import chatService, { IQueryTableDataParams } from '@/service/chat';
import miscServices from '@/service/misc';
import { AnswerPartsStatus, AnswerPartsType } from '@/constants/chat';
import { endAnswer } from '@/blocks/Chat/chatTaskProcessing/endAnswer';
import { useChatStore } from '@/store/chat';

export const chatGenerateDashboard = (props: IQueryTableDataParams, questionId, isStep) => {
  const { appendAnswerParts, updateAnswerId } = useChatStore.getState();

  chatService.queryAnswer({ questionId }).then((answerResponse) => {
    if (!answerResponse?.[0]?.questionId) {
      return;
    }
    const { questionId: answerQuestionId, id: answerId } = answerResponse[0];
    // update answerId
    updateAnswerId(answerQuestionId, answerId!);
    const params = {
      ...props,
      answerId,
      questionId: answerQuestionId,
    };
    if (isStep) {
      // Add loading effect of query data
      appendAnswerParts({
        data: {
          partType: AnswerPartsType.DATA,
          status: AnswerPartsStatus.LOADING,
          step: 2,
        },
        questionId: answerQuestionId,
      });
      // filter sql
      miscServices.characterHandler({ text: props.sql || '' }).then((filteredSql) => {
        params.sql = filteredSql;
        chatService.queryTableData(params).then((queryResult) => {
          appendAnswerParts({
            data: {
              ...queryResult,
              status: AnswerPartsStatus.FINISH,
              step: 2,
            },
            questionId: answerQuestionId,
          });

          appendAnswerParts({
            data: {
              partType: AnswerPartsType.DASHBOARD,
              status: AnswerPartsStatus.LOADING,
              step: 3,
            },
            questionId: answerQuestionId,
          });

          // The third step is to generate a chart
          dashboardChatThirdStep(params, answerQuestionId);
        });
      });
    } else {
      endAnswer(answerQuestionId, params, 2);
    }
  });
};

// The third step
export const dashboardChatThirdStep = (props: IQueryTableDataParams, questionId) => {
  const { appendAnswerParts } = useChatStore.getState();
  const getRecommendListParams = {
    ...props,
    tableList: undefined,
  };
  chatService.queryChart(props).then((res) => {
    if (!res.partType) {
      appendAnswerParts({
        data: {
          partType: AnswerPartsType.DASHBOARD,
          status: AnswerPartsStatus.FINISH,
          step: 3,
        },
        questionId,
      });
      endAnswer(questionId, getRecommendListParams, 3);
      return;
    }
    appendAnswerParts({
      data: {
        ...res,
        status: AnswerPartsStatus.FINISH,
        step: 3,
      },
      questionId,
    });
    endAnswer(questionId, getRecommendListParams, 4);
  });
};
