import createRequest from './base';
import { IAIModel, IInviteQrCode, ILoginAndQrCode, IRemainingUse } from '@/typings/ai';

const prefix = '/api/ai';

const getRemainingUse = createRequest<void, IRemainingUse>('/api/ai/config/remaininguses', {
  errorLevel: false,
});

const getLoginQrCode = createRequest<{ token?: string }, ILoginAndQrCode>('/api/ai/config/getLoginQrCode');

const getLoginStatus = createRequest<{ token?: string }, ILoginAndQrCode>('/api/ai/config/getLoginStatus', {
  errorLevel: false,
});

const getInviteQrCode = createRequest<void, IInviteQrCode>('/api/ai/config/getInviteQrCode');

// /api/ai/embedding/datasource
/**
 * Synchronize data sources to AI
 */
const syncDataBase = createRequest<{ dataSourceId: number; databaseName?: string; schemaName?: string }>(
  `${prefix}/embedding/datasource`,
  {
    method: 'post',
  },
);

// /api/v2/ai/model/list
/**
 * Get model list
 */
const getModelList = createRequest<void, IAIModel[]>('/api/v2/ai/model/list');

// Get MCP configuration
const getMcpConfig = createRequest<void, string>('/api/mcp/config/copy');

export default {
  getRemainingUse,
  getLoginQrCode,
  getLoginStatus,
  getInviteQrCode,
  syncDataBase,
  getModelList,
  getMcpConfig,
};
