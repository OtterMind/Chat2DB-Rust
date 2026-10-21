import { IDatabaseItem } from '../../typing';

// getDatabaseList
export interface DatabaseState {
  databaseList: IDatabaseItem[];
}

export const initDatabaseState: DatabaseState = {
  databaseList: [],
};
