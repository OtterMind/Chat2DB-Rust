import { ICommandLineRequestListItem } from '@/service/commandLine/commandLine';

export interface RequestState {
  commandLineRequestList: {
    [key: string]: ICommandLineRequestListItem;
  };
}

export const initialRequestState: RequestState = {
  commandLineRequestList: {},
};
