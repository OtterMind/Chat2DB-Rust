import { StateCreator } from 'zustand';
import { AIStore } from '../../store';

export interface PanelAction {
  setSize: (size: number) => void;
  setShowPanel: (showPanel: boolean) => void;
  togglePanel: () => void;
}

export const createPanelAction: StateCreator<AIStore, [['zustand/devtools', never]], [], PanelAction> = (set, _get) => ({
  setSize: (size) => {
    set({ size });
  },
  setShowPanel: (showPanel) => {
    set({ showPanel });
  },

  togglePanel: () => {
    set((state) => ({ showPanel: !state.showPanel }));
  },
});
