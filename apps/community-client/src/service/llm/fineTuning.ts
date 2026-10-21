import createRequest from '../base';
import { ILLMStartup } from '@/typings/llm';

const createFineTuning = createRequest<Omit<ILLMStartup, string>, void>('/api/ai/llm/ft/update', {
  method: 'post',
});

const updateFineTuning = createRequest<Omit<ILLMStartup, string>, void>('/api/ai/llm/ft/update', {
  method: 'post',
});

const getFineTuningList = createRequest<
  {
    organizationId: number;
    searchKey?: string;
  },
  ILLMStartup[]
>('/api/ai/llm/ft/list', {});

const getFineTuningDetail = createRequest<{ id: number }, ILLMStartup>('/api/ai/llm/ft', {});

// Start fine-tuning
const trainFineTuning = createRequest<{ id: number }, void>('/api/ai/llm/ft/train', {
  method: 'post',
});

// Start review
const evalFineTuning = createRequest<{ id: number }, void>('/api/ai/llm/ft/eval', {
  method: 'post',
});

export default {
  createFineTuning,
  updateFineTuning,
  getFineTuningList,
  getFineTuningDetail,
  trainFineTuning,
  evalFineTuning,
};
