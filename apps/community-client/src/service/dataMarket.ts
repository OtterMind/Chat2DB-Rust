import { IPageParams, IPageResponse } from '@/typings';
import createRequest from './base';
import { ICreateCollectionRequest, IDataSourceCollection } from '@/typings/dataMarket';

const prefix = '/api/ai/data/collection';

/** Create a data source collection */
const createCollection = createRequest<ICreateCollectionRequest, number>(`${prefix}/create`, {
  method: 'post',
});

/** Update data source collection */
const updateCollection = createRequest<ICreateCollectionRequest, number>(`${prefix}/update`, {
  method: 'post',
});

/** Get data collection details */
const queryDataMarketDetail = createRequest<{ id: number }, IDataSourceCollection>(`${prefix}/get`);

/** Query the data collection list */
const queryDataMarketList = createRequest<IPageParams, IPageResponse<IDataSourceCollection>>(`${prefix}/list`);

export default {
  createCollection,
  updateCollection,
  queryDataMarketDetail,
  queryDataMarketList,
};
