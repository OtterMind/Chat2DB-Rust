import {
  AnswerParts,
  AnswerVO,
  ChatExamplePrompt,
  ChatVO,
  PromptTableVO,
  UpdateAnswerPartsParams,
} from '@/typings/chat';
import createRequest from './base';
import { IPageParams, IPageResponse, IExcelSheetDataVO } from '@/typings/common';
import { QuestionType, ChatSourceType } from '@/constants/chat';

const prefix = '/api/ai/chat';

/** Query session list */
const getChatList = createRequest<IPageParams, IPageResponse<ChatVO>>(`/api/v2/ai/chat/list`);

/** Create session */
const createNewChat = createRequest<ChatVO, ChatVO>(`${prefix}/create`, {
  method: 'post',
});

/** Update session information, including the title and dataset. */
const updateChatInfo = createRequest<ChatVO, boolean>(`${prefix}/update`, {
  method: 'post',
});

/** View conversation details by ID */
const getChatDetailById = createRequest<
  {
    lastQuestionId?: any;
    id: number;
    pageSize: number;
  },
  ChatVO
>(`/api/v2/ai/chat/get`);

/** View conversation details by sharing a viewing connection */
const getChatShareViewDetail = createRequest<{ viewShareId: string }, ChatVO>(`${prefix}/share/view`);

/** View conversation details by sharing an edit link */
const getChatShareEditDetail = createRequest<{ editShareId: string }, ChatVO>(`${prefix}/share/edit`);

/** Delete conversation */
const deleteChat = createRequest<{ id: number }, boolean>(`${prefix}/delete`, {
  method: 'delete',
});

/** Updated dialogue answers */
const updateChatAnswer = createRequest<any, boolean>(`${prefix}/update/answer`, {
  method: 'post',
});

/** Request answer */
const queryAnswer = createRequest<{ questionId: number }, AnswerVO[]>(`${prefix}/list/answer`);

/** Feedback of like or dislike */
const chatFeedback = createRequest<{ id: number; like?: boolean; dislike?: boolean }, boolean>(
  `${prefix}/answer/feedback`,
  {
    method: 'post',
  },
);

/** Query mapping table */
const queryTables = createRequest<
  {
    message: string;
    dataSourceCollectionId?: number;
    dataSourceId?: number;
    databaseName?: string;
    schemaName?: string;
  },
  PromptTableVO[]
>('/api/ai/magic/prompt/mapping/tables', {
  method: 'post',
});

/** Query sample prompt */
const queryChatExamplePrompt = createRequest<any, ChatExamplePrompt[]>('/api/ai/magic/prompt/example');

/** Parse excel */
const excelCheck = createRequest<
  {
    filePath: string;
    fileName?: string;
  },
  {
    sheetList: IExcelSheetDataVO[];
    filePath: string;
  }
>('/api/ai/excel/check', {
  method: 'post',
});

export interface IQueryTableDataParams {
  /**
   * Conversation ID, must be passed when opening in AI conversation page
   */
  chatId?: number;
  /**
   * Data source collection ID
   */
  dataSourceCollectionId?: number;
  /**
   * Enter message
   */
  message: null | string;
  /**
   * Question type
   */
  questionType?: QuestionType;
  /**
   * Source
   */
  source?: ChatSourceType;
  /**
   * Query SQL
   */
  sql?: null | string;
  /**
   *Table list
   */
  tableList: PromptTableVO[];
}

// The second step of Excel or report query, query the table
const queryTableData = createRequest<IQueryTableDataParams, AnswerParts>('/api/v1/ai/slash_magic/query_table', {
  method: 'post',
});

// The third step of Excel or report query, query the table
const queryChart = createRequest<IQueryTableDataParams, AnswerParts>('/api/v1/ai/slash_magic/query_chart', {
  method: 'post',
});

// Update conversation answers every step of the way
const updateAnswerParts = createRequest<UpdateAnswerPartsParams, boolean>(`/api/ai/parts/answer/update`, {
  method: 'post',
  errorLevel: false,
});

/**
 * Get the conversation information concisely through dataSourceId
 * /api/ai/chat/getChatBrief
 */
const getChatBriefByDataSourceId = createRequest<
  {
    dataSourceId: number;
  },
  ChatVO
>('/api/v2/ai/chat/getChatBrief');

export default {
  createNewChat,
  getChatList,
  getChatDetailById,
  getChatShareViewDetail,
  getChatShareEditDetail,
  updateChatInfo,
  deleteChat,
  updateChatAnswer,
  queryAnswer,
  queryTables,
  chatFeedback,
  queryChatExamplePrompt,
  excelCheck,
  queryTableData,
  queryChart,
  updateAnswerParts,
  getChatBriefByDataSourceId,
};
