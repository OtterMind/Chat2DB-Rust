import { devtools } from 'zustand/middleware';
import { shallow } from 'zustand/shallow';
import { createWithEqualityFn } from 'zustand/traditional';
import { StateCreator } from 'zustand/vanilla';

interface ZoerState {
  // requested database
  zoerBoundInfo: {
    dataSourceId: number;
    databaseName?: string;
    schemaName?: string;
    databaseType: string;
  } | null;
}

const initialState: ZoerState = {
  zoerBoundInfo: null,
};

export interface ZoerAction {
  setZoerBoundInfo: (boundInfo: ZoerState['zoerBoundInfo']) => void;
  clearZoerBoundInfo: () => void;
}

export type ZoerStore = ZoerState & ZoerAction;

export const createUserAction: StateCreator<ZoerStore, [['zustand/devtools', never]], [], ZoerAction> = (set, _get) => ({
  setZoerBoundInfo: (boundInfo) => {
    set({ zoerBoundInfo: boundInfo });
  },
  clearZoerBoundInfo: () => {
    set({ zoerBoundInfo: null });
  },
});

const createStore: StateCreator<ZoerStore, [['zustand/devtools', never]]> = (...parameters) => ({
  ...initialState,
  ...createUserAction(...parameters),
});

export const useZoerStore = createWithEqualityFn<ZoerStore>()(
  devtools(createStore, {
    name: 'Chat2DB_User_Store',
  }),
  shallow,
);

// Clean store
export const clearZoerStore = () => {
  useZoerStore.setState(initialState);
};
