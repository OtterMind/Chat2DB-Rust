import * as VTable from '@visactor/vtable';

const onChangeCellValue = (tableInstance: VTable.ListTable, handleCellValueChange) => {
  const id = tableInstance.on('change_cell_value', (event) => {
    if (event.currentValue === event.changedValue) {
      return;
    }
    const { row, col } = event;
    const originData = tableInstance.getRecordByCell(col, row);
    const cellMeta = originData?.__CHAT2DB_CELL_META__?.[col];
    if (cellMeta?.largeValue) {
      tableInstance.changeCellValue(col, row, event.currentValue);
      return;
    }
    const headerField = tableInstance.getHeaderField(col, row);

    const rowId = originData.CHAT2DB_ROW_NUMBER;
    handleCellValueChange({
      ...event,
      rowId,
      field: headerField,
    });
  });
  return id;
};

export default onChangeCellValue;
