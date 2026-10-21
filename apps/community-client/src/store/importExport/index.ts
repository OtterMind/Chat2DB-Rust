import { devtools } from 'zustand/middleware';
import { shallow } from 'zustand/shallow';
import { createWithEqualityFn } from 'zustand/traditional';
import { StateCreator } from 'zustand/vanilla';
import { IDatabaseBaseInfo } from '@/typings/database';
import { ImportExportDataBoundInfo, ImportExportTaskDetails } from '@/typings/importExport';
import { ImportExportTaskStatus } from '@/constants/importExport';
import importExportServices from '@/service/importExport';

interface ImportExportState {
  runSqlBoundInfo: IDatabaseBaseInfo | null;
  importExportDataBoundInfo: ImportExportDataBoundInfo | null;
  showExportToolbar: boolean;
  taskList: ImportExportTaskDetails[];
  getTaskListTimer: NodeJS.Timeout | null;
  currentTask: ImportExportTaskDetails | null;
  logModalTaskId: number | null;
}

const initialState: ImportExportState = {
  runSqlBoundInfo: null,
  importExportDataBoundInfo: null,
  showExportToolbar: false,
  taskList: [],
  getTaskListTimer: null,
  currentTask: null,
  logModalTaskId: null,
};

export interface ImportExportAction {
  setRunSqlBoundInfo: (data: ImportExportState['runSqlBoundInfo']) => void;
  setImportExportDataBoundInfo: (data: ImportExportState['importExportDataBoundInfo']) => void;
  clearImportExportStore: () => void;
  setShowExportToolbar: (showExportToolbar: ImportExportState['showExportToolbar']) => void;
  getTaskList: (param?: { visible: boolean }) => void;
  openLogModal: (taskId: number | null) => void;
}

export type ImportExportStore = ImportExportState & ImportExportAction;

// import { useImportExportStore } from '@/store/importExport';
// const { runSqlBoundInfo, setRunSqlBoundInfo } = useImportExportStore((state) => {
//   return {
//     runSqlBoundInfo: state.runSqlBoundInfo,
//     setRunSqlBoundInfo: state.setRunSqlBoundInfo,
//   };
// });

export const createImportExportAction: StateCreator<
  ImportExportStore,
  [['zustand/devtools', never]],
  [],
  ImportExportAction
> = (set, get) => ({
  setRunSqlBoundInfo: (_runSqlBoundInfo) => {
    set({
      runSqlBoundInfo: _runSqlBoundInfo,
    });
  },
  setImportExportDataBoundInfo: (_importExportDataBoundInfo) => {
    set({
      importExportDataBoundInfo: _importExportDataBoundInfo,
    });
  },
  clearImportExportStore: () => {
    set(initialState);
  },
  setShowExportToolbar: (showExportToolbar) => {
    if (showExportToolbar) {
      get().getTaskList();
    }
    set({ showExportToolbar });
  },
  getTaskList: (params) => {
    const { visible } = params || {};
    // clear timer
    const { getTaskListTimer } = get();
    if (getTaskListTimer) {
      clearTimeout(getTaskListTimer);
    }
    importExportServices.getTaskList({ pageNo: 1, pageSize: 10 }).then((res) => {
      if (!res.data) {
        return;
      }
      const _taskList = res.data || [];
      const _currentTask = _taskList.find((item) =>
        [ImportExportTaskStatus.INIT, ImportExportTaskStatus.PROCESSING, ImportExportTaskStatus.RUNNING].includes(
          item.taskStatus,
        ),
      );
      set({ currentTask: _currentTask, taskList: _taskList });
      if (_currentTask) {
        set({
          showExportToolbar: visible ? true : get().showExportToolbar,
        });
      }
    });
  },
  openLogModal: (taskId) => {
    set({ logModalTaskId: taskId });
  },
});

const createStore: StateCreator<ImportExportStore, [['zustand/devtools', never]]> = (...parameters) => ({
  ...initialState,
  ...createImportExportAction(...parameters),
});

export const useImportExportStore = createWithEqualityFn<ImportExportStore>()(
  devtools(createStore, {
    name: 'Chat2DB_ImportExport_Store',
  }),
  shallow,
);

// // Clean up the store
export const clearImportExportStore = () => {
  useImportExportStore.setState(initialState);
};
