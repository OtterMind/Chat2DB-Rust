import { useCommonStore } from './index';
export interface ICopyFocusedContent {
  focusedContent: any[][] | any[] | string | null;
}

export const initCopyFocusedContent = {
  focusedContent: null,
};

export const setFocusedContent = (focusedContent) => {
  useCommonStore.setState({ focusedContent });
};

export const getFocusedContent = () => {
  return useCommonStore.getState().focusedContent;
};
