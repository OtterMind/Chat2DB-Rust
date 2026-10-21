import { create } from 'zustand';
import { initCopyFocusedContent, ICopyFocusedContent } from './copyFocusedContent';
import { initComponentsContent, IComponentsContent } from './components';

export type IStore = ICopyFocusedContent & IComponentsContent;

export const useCommonStore = create(() => ({
  ...initCopyFocusedContent,
  ...initComponentsContent,
}));

export const clearCommonStore = () => {
  useCommonStore.setState({
    ...initCopyFocusedContent,
    // ...initComponentsContent,
  });
};
