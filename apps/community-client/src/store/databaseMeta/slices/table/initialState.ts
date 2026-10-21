import { ITableItem } from '../../typing';

export interface TableState {
  tableList: ITableItem[];
}

export const initTableState: TableState = {
  tableList: [],
};
