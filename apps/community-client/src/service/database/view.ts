import createRequest from '../base';
import { IDBContextInfo } from '@/typings/database';

const getViewMeta = createRequest<IDBContextInfo & { viewName: string }, any>('/api/rdb/view/view_meta', {});

export default {
  getViewMeta,
};
