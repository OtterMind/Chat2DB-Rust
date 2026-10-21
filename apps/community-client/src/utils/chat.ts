// Revise the old data structure of answer in chat
import { AnswerVO, ChatDetailVO } from '@/typings/chat';
import { reverseFormattedSqlExecuteData } from './dashboard';
import { AnswerPartsStatus, AnswerPartsType } from '@/constants/chat';

// Revise the data structure of each answerDetail
export const revisalChatAnswerDetail = (answerDetail: AnswerVO): AnswerVO => {
  if (!answerDetail.parts && answerDetail.content) {
    const parts: any = [];
    try {
      const contents = JSON.parse(answerDetail.content);
      contents.map((item) => {
        let chartSchema: any = null;
        let metaData: any = null;
        chartSchema = JSON.parse(item.schema);
        metaData = reverseFormattedSqlExecuteData(chartSchema.data);
        chartSchema.title = item.name;
        delete chartSchema.data;
        parts.push({
          databaseInfo: {
            connectable: item.connectable,
            dataSourceId: item.dataSourceId,
            databaseName: item.databaseName,
            databaseType: item.databaseType,
            sql: item.ddl,
          },
          metaData: metaData,
          chartSchema: chartSchema,
          partType: AnswerPartsType.DASHBOARD,
          status: AnswerPartsStatus.FINISH,
        });
      });
      answerDetail.parts = parts;
    } catch (error) {
      console.error('Failed to migrate legacy chat answer content', error);
    }
    return answerDetail;
  }
  return answerDetail;
};

// Revise the data structure of chatDetail
export const revisalChatDetail = (chatDetails: ChatDetailVO): ChatDetailVO => {
  // if (!chatDetails.answers) {
  //   return chatDetails
  // }

  // chatDetails.answers = chatDetails.answers.map(revisalChatAnswerDetail)
  return chatDetails;
};
