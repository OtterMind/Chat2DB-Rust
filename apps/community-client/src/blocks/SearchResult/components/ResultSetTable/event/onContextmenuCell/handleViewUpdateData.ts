import * as VTable from '@visactor/vtable';
import { ISelectEvent } from '../../typings';

// View and modify current data
const handleViewUpdateData = (tableInstance: VTable.ListTable, selectEvent: ISelectEvent) => {
  const cells = selectEvent;
  const { col, row } = cells;
  const record = tableInstance.getRecordByCell(col, row);
  const cellMeta = record?.__CHAT2DB_CELL_META__?.[col];
  return { col, row, tableInstance, cellMeta };
};

export default handleViewUpdateData;
