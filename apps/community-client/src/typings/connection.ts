import { DatabaseTypeCode } from '@/constants';

export enum DataSourceStorageType {
  CLOUD = 'CLOUD',
  LOCAL = 'LOCAL',
}

// Connect to advanced configuration list information
export interface IConnectionExtendInfoItem {
  key: string;
  value: string;
}

// Connected environment information
export interface IConnectionEnv {
  id: number;
  name: string;
  shortName: string;
  color: string;
}

export interface IConnectionDetails {
  spaceId: number;
  id: number;
  alias: string;
  environment: IConnectionEnv;
  type: DatabaseTypeCode;

  isAdmin: boolean;
  url: string;
  user: string;
  password: string;
  ConsoleOpenedStatus: 'y' | 'n';
  extendInfo: IConnectionExtendInfoItem[];
  environmentId: number;
  storageType: DataSourceStorageType;
  ssh: any;
  driverConfig: {
    jdbcDriver: string;
    jdbcDriverClass: string;
  };
  [key: string]: any;
}

export interface IConnectionListItem {
  id: number;
  alias: string;
  environment: IConnectionEnv;
  type: DatabaseTypeCode;
  supportDatabase: boolean;
  supportSchema: boolean;
}

export type ICreateConnectionDetails = Omit<IConnectionDetails, 'id'>;
