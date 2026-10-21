import { StateCreator } from 'zustand';
import { DatabaseMetaStore } from '../../store';
import { IDataSourceItem } from '../../typing';

export interface DataSourceAction {
  /** Set dataSource list */
  setDataSourceList: (dataSourceList: IDataSourceItem[]) => void;
}

export const createDataSourceAction: StateCreator<
  DatabaseMetaStore,
  [['zustand/devtools', never]],
  [],
  DataSourceAction
> = (set) => ({
  setDataSourceList: (dataSourceList) => {
    set({ dataSourceList });
  },
});
