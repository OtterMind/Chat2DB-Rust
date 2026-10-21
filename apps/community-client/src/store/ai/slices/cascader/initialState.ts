import { IAICascaderData } from '@/blocks/AI/components/AICascaderSource';

export type PageType = 'workspace' | 'dashboard' | 'chat' | 'stream';

export interface CascaderState {
  // Map to store cascader data for different pages
  cascaderDataMap: Record<PageType, IAICascaderData>;
}

export const initCascaderState: CascaderState = {
  cascaderDataMap: {
    stream: null,
    workspace: null,
    dashboard: null,
    chat: null,
  },
};
