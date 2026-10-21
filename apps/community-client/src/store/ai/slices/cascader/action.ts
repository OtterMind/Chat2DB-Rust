import { StateCreator } from 'zustand';
import { AIStore } from '../../store';
import { IAICascaderData } from '@/blocks/AI/components/AICascaderSource';
import { PageType } from './initialState';

export interface CascaderAction {
  setCascaderData: (pageType: PageType, data: IAICascaderData) => void;
  getCascaderData: (pageType: PageType) => IAICascaderData | null;
  clearCascaderData: (pageType: PageType) => void;
}

export const createCascaderAction: StateCreator<AIStore, [['zustand/devtools', never]], [], CascaderAction> = (
  set,
  get,
) => ({
  setCascaderData: (pageType: PageType, data: IAICascaderData) => {
    set((state) => ({
      cascaderDataMap: {
        ...state.cascaderDataMap,
        [pageType]: data,
      },
    }));
  },

  getCascaderData: (pageType: PageType) => {
    return get().cascaderDataMap[pageType] || null;
  },

  clearCascaderData: (pageType: PageType) => {
    set((state) => ({
      cascaderDataMap: {
        ...state.cascaderDataMap,
        [pageType]: null,
      },
    }));
  },
});
