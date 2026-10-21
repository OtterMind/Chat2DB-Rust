import createRequest from './base';
import { IERTableDetail } from '@/typings/er';

/** Get ER chart information */
const getTableErInfo = createRequest<
  { dataSourceId?: number; databaseName?: string; schemaName?: string },
  {
    tables: IERTableDetail[];
    position: string;
  }
>('/api/er/get_info', { method: 'get' });

/** Save ER chart position */
const saveTableErPosition = createRequest<
  { dataSourceId?: number; databaseName?: string; schemaName?: string; position: string },
  void
>('/api/er/save_position', { method: 'post', errorLevel: false });

export { getTableErInfo, saveTableErPosition };
