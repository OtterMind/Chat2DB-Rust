export interface SelectedModelOption {
  value: string;
  label: string;
}

export interface ModelState {
  modelList: Array<{
    label: string;
    value: string;
    isDefault: boolean;
  }>;
  selectedModel: SelectedModelOption | null;
}

export const initModelState: ModelState = {
  modelList: [],
  selectedModel: null,
};
