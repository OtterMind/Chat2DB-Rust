import type { StateCreator } from 'zustand/vanilla';
import { WorkspaceStore } from '../../store';
import { CommonState } from './initialState';
import { useGlobalStore } from '@/store/global';
import jcefApi from '@/jcef';
import { randomLargeLong } from '@/utils';
import { WorkspaceTabType } from '@/constants';

export interface CommonAction {
  setCurrentConnectionDetails: (data: CommonState['currentConnectionDetails']) => void;
  setCurrentWorkspaceExtend: (workspaceExtend: CommonState['currentWorkspaceExtend']) => void;
  setCurrentWorkspaceGlobalExtend: (workspaceGlobalExtend: CommonState['currentWorkspaceGlobalExtend']) => void;
  readFile: (filePath: string, fileExtension?: string) => void;
}

export const createCommonAction: StateCreator<WorkspaceStore, [['zustand/devtools', never]], [], CommonAction> = (
  set,
  get,
) => ({
  setCurrentConnectionDetails: (data) => {
    set({ currentConnectionDetails: data });
  },
  setCurrentWorkspaceExtend: (workspaceExtend) => {
    set({ currentWorkspaceExtend: workspaceExtend });
  },
  setCurrentWorkspaceGlobalExtend: (workspaceGlobalExtend) => {
    set({ currentWorkspaceGlobalExtend: workspaceGlobalExtend });
  },
  readFile: (filePath: string, fileExtension?: string) => {
    jcefApi.readFile(filePath).then((fileContent) => {
      useGlobalStore.getState().setMainPageActiveTab({ page: 'workspace' });
      const workspaceTabList = get().workspaceTabList;
      if (workspaceTabList?.some((tab) => tab.uniqueData?.filePath === filePath)) {
        const tab: any = workspaceTabList.find((_tab) => _tab.uniqueData?.filePath === filePath);
        if (tab) {
          tab.uniqueData = {
            ...tab.uniqueData,
            filePath,
            fileExtension: fileExtension || tab.uniqueData?.fileExtension,
            ddl: fileContent,
          };
          get().setActiveConsoleId(tab.id);
          get().setWorkspaceTabList([...workspaceTabList]);
        }
      } else {
        setTimeout(() => {
          get().addWorkspaceTab({
            id: randomLargeLong(),
            type: WorkspaceTabType.LocalSQLFile,
            title: filePath,
            uniqueData: {
              filePath,
              fileExtension,
              ddl: fileContent,
            },
          });
        }, 0);
      }
    });
  },
});
