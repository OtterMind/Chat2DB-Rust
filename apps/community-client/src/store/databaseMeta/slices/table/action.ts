import { StateCreator } from 'zustand';
import { ITableItem } from '../../typing';
import { DatabaseMetaStore } from '../../store';

export interface TableAction {
  setTableList: (tableList: ITableItem[]) => void;
}

export const createTableAction: StateCreator<DatabaseMetaStore, [['zustand/devtools', never]], [], TableAction> = (
  set,
) => ({
  setTableList: (tableList) => {
    set({ tableList });
  },
});
