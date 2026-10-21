import { ISchemaItem } from '../../typing';

export interface SchemaState {
  schemaList: ISchemaItem[];
}

export const initSchemaState: SchemaState = {
  schemaList: [],
};
