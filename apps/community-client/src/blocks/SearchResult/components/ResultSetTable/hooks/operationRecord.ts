import { ICellChangeRecord } from '../typings';

export function buildUpdateOperation(rowId: string, originData: Record<string, any>, records: ICellChangeRecord[]) {
  const oldDataList = {
    ...originData,
  };
  const dataList = {
    ...originData,
  };

  records
    .filter((record) => record.rowId === rowId)
    .forEach((record) => {
      oldDataList[record.field] = record.currentValue;
      dataList[record.field] = record.changedValue;
    });

  return {
    rowId,
    type: 'UPDATE',
    dataList,
    oldDataList,
  };
}
