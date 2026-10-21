import { StateCreator } from 'zustand';
import { IDatabaseItem } from '../../typing';
import { DatabaseMetaStore } from '../../store';

export interface DatabaseAction {
  setDatabaseList: (databaseList: IDatabaseItem[]) => void;
}

export const createDatabaseAction: StateCreator<
  DatabaseMetaStore,
  [['zustand/devtools', never]],
  [],
  DatabaseAction
> = (set) => ({
  setDatabaseList: (databaseList) => {
    set({ databaseList });
  },
});
