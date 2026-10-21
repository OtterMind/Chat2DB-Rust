import createRequest from '../base';
import { RedisDataItem, RedisKeyScanResult } from '@/typings/redis';

type DataSourceId = string | number;

export interface CreateRedisDataParams extends RedisDataItem {
  dataSourceId: DataSourceId;
  databaseName: string;
}

export interface UpdateRedisDataParams {
  dataSourceId: DataSourceId;
  databaseName: string;
  oldRedisKey: RedisDataItem;
  newRedisKey: RedisDataItem;
}

export interface DeleteRedisDataParams {
  dataSourceId: DataSourceId;
  databaseName: string;
  keyName: string | null;
}

export interface QueryRedisDataParams {
  dataSourceId: DataSourceId;
  databaseName: string;
  searchKey?: string;
}

export interface ScanRedisKeysParams {
  dataSourceId: DataSourceId;
  databaseName: string;
  searchKey?: string;
  cursor?: string;
  count?: number;
}

export interface QueryRedisKeyDetailParams {
  dataSourceId: DataSourceId;
  databaseName: string;
  keyName: string;
}

const createRedisData = createRequest<CreateRedisDataParams, RedisDataItem>('/api/redis/create', { method: 'post' });
const updateRedisData = createRequest<UpdateRedisDataParams, RedisDataItem>('/api/redis/update', { method: 'post' });
const deleteRedisData = createRequest<DeleteRedisDataParams, boolean>('/api/redis/delete', { method: 'post' });
const queryRedisData = createRequest<QueryRedisDataParams, RedisDataItem[]>('/api/redis/query', { method: 'get' });
const scanRedisKeys = createRequest<ScanRedisKeysParams, RedisKeyScanResult>('/api/redis/keys', { method: 'get' });
const queryRedisKeyDetail = createRequest<QueryRedisKeyDetailParams, RedisDataItem>('/api/redis/key_detail', {
  method: 'get',
});

export default {
  createRedisData,
  updateRedisData,
  deleteRedisData,
  queryRedisData,
  scanRedisKeys,
  queryRedisKeyDetail,
};
