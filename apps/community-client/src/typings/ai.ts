import { IDatabaseBaseInfo } from './database';
import { DatabaseTypeCode } from '@/constants';
import { PromptTableVO } from './chat';
import { ChatSourceType } from '@/constants/chat';

export enum AIType {
  CHAT2DBAI = 'CHAT2DBAI',
  ZHIPUAI = 'ZHIPUAI',
  BAICHUANAI = 'BAICHUANAI',
  WENXINAI = 'WENXINAI',
  // TONGYIQIANWENAI='TONGYIQIANWENAI',
  OPENAI = 'OPENAI',
  AZUREAI = 'AZUREAI',
  RESTAI = 'RESTAI',
}

export interface IRemainingUse {
  key: string;
  wechatMpUrl: string;
  expiry: number;
  remainingUses: number;
}

export interface ILoginAndQrCode {
  token: string;
  wechatQrCodeUrl: string;
  apiKey: string;
  tip: string;
}

export interface IInviteQrCode {
  wechatQrCodeUrl: string;
  tip: string;
}

export interface IAIConfig {
  aiSqlSource: AIType;
  apiKey?: string;
  apiHost?: string;
  httpProxyHost?: string;
  httpProxyPort?: string;
  stream?: boolean;
  secretKey?: string;
  model?: string;
}

// Natural language to SQL
export interface TextToSQLParams {
  /**
   * Conversation ID, must be passed when opening in AI conversation page
   */
  chatId?: number;
  /**
   * Data source ID, must be passed when used in the console window
   */
  dataSourceId?: number;
  /**
   * Enter message
   */
  message: string;
  /**
   * Source
   */
  source: ChatSourceType;
  /**
   * List of table names
   */
  tableList?: PromptTableVO[];
  /**
   *Data collection id
   */
  dataSourceCollectionId?: number;
}

// Convert natural language with a dataset ID to SQL.
export interface TextToSQLWithCollectionIdParams {
  dataSourceCollectionId: number;
  message: string;
  source: ChatSourceType;
}

// Natural language creation table
export interface TextToCreateTableParams {
  tableName: string;
  columnList: string;
  databaseType: DatabaseTypeCode;
}

// Natural language modification table
export interface TextToAlterTableParams extends IDatabaseBaseInfo {
  message: string;
}

// AI dialogue obtains input parameters of token
export type GetChatTokenParams = TextToSQLParams | TextToCreateTableParams | TextToAlterTableParams;

// AI dialogue obtains the return value of token
export interface GetChatTokenResponse {
  /**
   * Conversation ID
   */
  chatId: number;
  /**
   *IssueId
   */
  questionId: number;
  /**
   * token
   */
  token: string;
}

export interface IAIModel {
  /**
   * Model type
   */
  modelType: string;
  /**
   * Model name
   */
  modelName: string;
  /**
   * Model display name
   */
  displayName: string;
  /**
   * Whether it is the default model
   */
  isDefault: boolean;
}
