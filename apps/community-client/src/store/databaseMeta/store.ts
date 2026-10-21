import { StateCreator, create } from 'zustand';
import { devtools } from 'zustand/middleware';
import { DatabaseMetaState, initialState } from './initialState';
import { DataSourceAction, createDataSourceAction } from './slices/datasource/action';
import { DatabaseAction, createDatabaseAction } from './slices/database/action';
import { SchemaAction, createSchemaAction } from './slices/schema/action';
import { FieldAction, createFieldAction } from './slices/field/action';
import { TableAction, createTableAction } from './slices/table/action';

export type DatabaseMetaAction = DataSourceAction & DatabaseAction & SchemaAction & TableAction & FieldAction;
export type DatabaseMetaStore = DatabaseMetaState & DatabaseMetaAction;

const createStore: StateCreator<DatabaseMetaStore, [['zustand/devtools', never]]> = (...parameters) => ({
  ...initialState,
  ...createDataSourceAction(...parameters),
  ...createDatabaseAction(...parameters),
  ...createSchemaAction(...parameters),
  ...createTableAction(...parameters),
  ...createFieldAction(...parameters),
});
export const useDatabaseMetaStore = create(
  devtools(createStore, {
    name: 'Chat2DB_Chat_Store',
  }),
);
