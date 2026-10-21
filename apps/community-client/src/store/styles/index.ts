/* https://github.com/ant-design/antd-style/issues/176 */

import { devtools } from 'zustand/middleware';
import { shallow } from 'zustand/shallow';
import { createWithEqualityFn } from 'zustand/traditional';
import { StateCreator } from 'zustand/vanilla';
import { Theme } from 'antd-style';

interface StylesState {
  theme: Omit<Theme, 'prefixCls'> | null;
}

const initialState: StylesState = {
  theme: null,
};

export interface StylesAction {
  setTheme: (theme: Omit<Theme, 'prefixCls'>) => void;
}

export type StylesStore = StylesState & StylesAction;

export const createStylesAction: StateCreator<StylesStore, [['zustand/devtools', never]], [], StylesAction> = (
  set,
) => ({
  setTheme: (theme) => {
    set({ theme });
  },
});

const createStore: StateCreator<StylesStore, [['zustand/devtools', never]]> = (...parameters) => ({
  ...initialState,
  ...createStylesAction(...parameters),
});

export const useStylesStore = createWithEqualityFn<StylesStore>()(
  devtools(createStore, {
    name: 'Chat2DB_Styles_Store',
  }),
  shallow,
);
