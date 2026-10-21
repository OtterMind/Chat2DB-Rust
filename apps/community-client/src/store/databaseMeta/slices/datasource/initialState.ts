import { IDataSourceItem } from '../../typing';

export interface DataSourceState {
  dataSourceList: IDataSourceItem[];
}

export const initDataSourceState: DataSourceState = {
  dataSourceList: [],
};
