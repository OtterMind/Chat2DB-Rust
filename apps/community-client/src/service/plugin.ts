import { IPluginDataPackageVO, IPluginItem } from '@/typings/plugin';
import createRequest from './base';

const queryPluginList = createRequest<void, IPluginItem[]>('/api/plugin/list_a', {});

const queryToken = createRequest<void, string>('/api/plugin/token', {
  method: 'get',
});

const queryPluginDataPackageList = createRequest<{ token: string }, IPluginDataPackageVO[]>(
  '/api/plugin/data_package/list_a',
  {},
);

const addPluginDownloadCount = createRequest<{ id: number }, void>('/api/plugin/add_download_a', {
  method: 'post',
});
export default {
  queryPluginList,
  queryToken,
  queryPluginDataPackageList,
  addPluginDownloadCount,
};
