import * as VTable from '@visactor/vtable';
import { copyToClipboard } from '@/utils';
import { setInternalResultGridClipboard } from '@/utils/internalClipboard';

export const copyResultGridSelection = (tableInstance: VTable.ListTable) => {
  // listens to the event of _tableInstance
  const selectedCells = tableInstance.getSelectedCellInfos();

  if (!selectedCells?.length) return null;

  // Find the row and column range of the selected area
  let maxRow = -1;
  let maxCol = -1;
  let minRow = Infinity;
  let minCol = Infinity;
  selectedCells.forEach((rowCells) => {
    rowCells.forEach((cell) => {
      maxRow = Math.max(maxRow, cell.row);
      maxCol = Math.max(maxCol, cell.col);
      minRow = Math.min(minRow, cell.row);
      minCol = Math.min(minCol, cell.col);
    });
  });

  // creates an empty two-dimensional array with the size of the actual size of the selected area.
  const resultArray: string[][] = Array(maxRow - minRow + 1)
    .fill(null)
    .map(() => Array(maxCol - minCol + 1).fill(''));

  // fills the selected cell data, you need to subtract the minimum row and column number to adjust the position
  selectedCells.forEach((rowCells) => {
    rowCells.forEach((cell) => {
      if (cell.value === null) {
        resultArray[cell.row - minRow][cell.col - minCol] = '';
      } else if (cell.value === '') {
        resultArray[cell.row - minRow][cell.col - minCol] = "''";
      } else {
        resultArray[cell.row - minRow][cell.col - minCol] = String(cell.value);
      }
    });
  });

  // Copy to clipboard
  if (copyToClipboard(resultArray)) {
    setInternalResultGridClipboard(resultArray);
    return resultArray.map((row) => row.join('\t')).join('\n');
  }
  return null;
};

const onCopyData = (tableInstance: VTable.ListTable) => {
  copyResultGridSelection(tableInstance);
};

export default onCopyData;
