import { StateCreator } from 'zustand';
import { IFieldItem } from '../../typing';
import { DatabaseMetaStore } from '../../store';

export interface FieldAction {
  setFieldList: (fieldList: IFieldItem[]) => void;
}

export const createFieldAction: StateCreator<DatabaseMetaStore, [['zustand/devtools', never]], [], FieldAction> = (
  set,
) => ({
  setFieldList: (fieldList) => {
    set({ fieldList });
  },
});
