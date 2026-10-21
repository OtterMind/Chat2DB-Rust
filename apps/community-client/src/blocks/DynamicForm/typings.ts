import { InputType } from './constants';

export interface IDynamicFormItem {
  // field name
  name: string;
  // field label name
  labelName: string;
  // field
  inputType: InputType;
  // field
  defaultValue?: string;
  // is a js expression string
  display?: string | null;
  // field multi-selectable?
  multiple?: boolean;
  // field required?
  required?: boolean;
  // drop-down option
  selects?: IDynamicFormSelect[];
}

export interface IDynamicFormSelect {
  label: string;
  value: string;
}
