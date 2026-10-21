import createRequest from '../base';
import { ILLMStartup, ILLMStartupListItem } from '@/typings/llm';

const createStartup = createRequest<Omit<ILLMStartup, string>, void>('/api/ai/llm/startup/update', {
  method: 'post',
});

const updateStartup = createRequest<Omit<ILLMStartup, string>, void>('/api/ai/llm/startup/update', {
  method: 'post',
});

const getStartupList = createRequest<
  {
    organizationId: number;
    searchKey?: string;
  },
  ILLMStartupListItem[]
>('/api/ai/llm/startup/list', {});

const getStartupDetail = createRequest<{ id: number }, ILLMStartup>('/api/ai/llm/startup', {});

const invokeStartup = createRequest<{ id: number }, void>('/api/ai/llm/startup/invoke', {
  method: 'post',
});

export default {
  createStartup,
  updateStartup,
  getStartupList,
  getStartupDetail,
  invokeStartup,
};
