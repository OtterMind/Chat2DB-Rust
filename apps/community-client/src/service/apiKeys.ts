import createRequest from './base';

export interface CreateApiKeyParams {
  authType?: string;
  name: string;
  endTime?: string;
  nonExpire: 'Y' | 'N';
}

const createApiKey = createRequest<CreateApiKeyParams, ApiKeyDetail>(`/api/key/create`, {
  method: 'post',
});

export interface ApiKeyDetail {
  id: number;
  authType?: string;
  apiKey: string;
  endTime?: string;
  modifyTime: string;
}

const getApiKeyList = createRequest<void, ApiKeyDetail[]>(`/api/key/list`, {
  method: 'get',
});

const deleteApiKey = createRequest<{ id: number }, void>(`/api/key/delete`, {
  method: 'delete',
});

export default { createApiKey, getApiKeyList, deleteApiKey };
