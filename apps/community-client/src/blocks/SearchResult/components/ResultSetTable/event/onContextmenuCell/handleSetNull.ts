import * as VTable from '@visactor/vtable';

const handleSetNull = (tableInstance: VTable.ListTable) => {
  const cells = tableInstance.getSelectedCellInfos() || [];
  cells.map((rowCells) => { 
    rowCells.map((cell) => {
      // does not change the meter header
      if (cell.row === 0) {
        return;
      }
      tableInstance.changeCellValue(cell.col, cell.row, null);
     });
  });
};

export default handleSetNull;
