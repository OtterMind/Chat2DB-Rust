import { DatabaseState, initDatabaseState } from './slices/database/initialState';
import { DataSourceState, initDataSourceState } from './slices/datasource/initialState';
import { FieldState, initFieldState } from './slices/field/initalState';
import { SchemaState, initSchemaState } from './slices/schema/initialState';
import { TableState, initTableState } from './slices/table/initialState';

export type DatabaseMetaState = DataSourceState & DatabaseState & SchemaState & TableState & FieldState;

export const initialState: DatabaseMetaState = {
  ...initDataSourceState,
  ...initDatabaseState,
  ...initSchemaState,
  ...initTableState,
  ...initFieldState,
};
