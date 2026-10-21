import { StateCreator } from 'zustand';
import { ISchemaItem } from '../../typing';
import { DatabaseMetaStore } from '../../store';

export interface SchemaAction {
  setSchemaList: (schemaList: ISchemaItem[]) => void;
}

export const createSchemaAction: StateCreator<DatabaseMetaStore, [['zustand/devtools', never]], [], SchemaAction> = (
  set,
) => ({
  setSchemaList: (schemaList) => {
    set({ schemaList });
  },
});
