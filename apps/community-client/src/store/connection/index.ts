import { UseBoundStoreWithEqualityFn, createWithEqualityFn } from 'zustand/traditional';
import { devtools } from 'zustand/middleware';
import { shallow } from 'zustand/shallow';
import { StoreApi } from 'zustand';

import { IConnectionListItem, IConnectionEnv } from '@/typings/connection';
import connectionService from '@/service/connection';

import { useWorkspaceStore } from '@/store/workspace';

export interface IConnectionStore {
  connectionList: IConnectionListItem[] | null;
  connectionEnvList: IConnectionEnv[] | null;
}

export const initConnectionStore = {
  connectionList: null,
  connectionEnvList: null,
};

export const useConnectionStore: UseBoundStoreWithEqualityFn<StoreApi<IConnectionStore>> = createWithEqualityFn(
  devtools(() => initConnectionStore),
  shallow,
);

export const setConnectionList = (connectionList: IConnectionListItem[]) => {
  return useConnectionStore.setState({ connectionList });
};

export const setConnectionEnvList = (connectionEnvList: IConnectionEnv[]) => {
  return useConnectionStore.setState({ connectionEnvList });
};

export const getConnectionList: () => Promise<IConnectionListItem[]> = () => {
  return new Promise((resolve, reject) => {
    const currentConnectionDetails = useWorkspaceStore.getState().currentConnectionDetails;
    const setCurrentConnectionDetails = useWorkspaceStore.getState().setCurrentConnectionDetails;
    connectionService
      .getList({
        pageNo: 1,
        pageSize: 1000,
        refresh: true,
      })
      .then((res) => {
        const connectionList = res?.data || [];
        useConnectionStore.setState({ connectionList });
        resolve(connectionList);

        // If the connection list is empty, set the current connection to empty
        if (connectionList.length === 0) {
          setCurrentConnectionDetails(null);
          return;
        }

        // If the current connection does not exist, set the current connection to the first connection
        if (!currentConnectionDetails?.id) {
          setCurrentConnectionDetails(connectionList[0]);
          return;
        }

        // If it exists but is not in the list, set the current connection to the first connection
        const currentConnection = connectionList.find((item) => item.id === currentConnectionDetails?.id);
        if (!currentConnection) {
          setCurrentConnectionDetails(connectionList[0]);
        }
      })
      .catch(() => {
        useConnectionStore.setState({ connectionList: [] });
        reject([]);
      });
  });
};

export const getConnectionEnvList = () => {
  connectionService.getEnvList().then((res) => {
    setConnectionEnvList(res);
  });
};

// clear connection store
export const clearConnectionStore = () => {
  useConnectionStore.setState(initConnectionStore);
};
