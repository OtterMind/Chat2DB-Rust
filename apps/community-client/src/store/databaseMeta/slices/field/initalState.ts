import { IFieldItem } from '../../typing';

export interface FieldState {
  fieldList: IFieldItem[];
}

export const initFieldState: FieldState = {
  fieldList: [],
};
