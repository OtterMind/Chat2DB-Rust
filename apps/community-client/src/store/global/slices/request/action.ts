import produce from 'immer';
import type { StateCreator } from 'zustand/vanilla';
import { GlobalStore } from '../../store';
import { ICommandLineRequestListItem } from '@/service/commandLine/commandLine';

export interface RequestAction {
  // add ICommandLineRequestListItem
  addCommandLineRequestListItem: (data: ICommandLineRequestListItem) => void;
  // remove ICommandLineRequestListItem
  removeCommandLineRequestListItem: (id: string) => void;
  // update ICommandLineRequestListItem
  updateCommandLineRequestListItem: (id: string, data: ICommandLineRequestListItem) => void;
}

export const createRequestAction: StateCreator<GlobalStore, [['zustand/devtools', never]], [], RequestAction> = (
  set,
) => ({
  addCommandLineRequestListItem: (data: ICommandLineRequestListItem) => {
    set(
      produce((state) => {
        state.commandLineRequestList[data.requestData.uuid] = data;
      }),
    );
  },
  removeCommandLineRequestListItem: (id: string) => {
    set(
      produce((state) => {
        delete state.commandLineRequestList[id];
      }),
    );
  },
  updateCommandLineRequestListItem: (id: string, data: ICommandLineRequestListItem) => {
    set(
      produce((state) => {
        state.commandLineRequestList[id] = data;
      }),
    );
  },
});
