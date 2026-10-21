import * as VTable from '@visactor/vtable';
import { readClipboard } from '@/utils/clipboard';
import { normalizeCreateRowPasteValues, normalizePasteTargetRange } from '@/blocks/SearchResult/utils';
import type { OperationRecordUtils } from '../../hooks/useOperationRecord';

type PasteOperationRecordUtils = Pick<OperationRecordUtils, 'isCreateRow'>;

function getSelectedPasteRange(tableInstance: VTable.ListTable, rangeIndex: number) {
  const range = tableInstance.stateManager.select.ranges[rangeIndex];
  return normalizePasteTargetRange({
    startCol: range.start.col,
    startRow: range.start.row,
    endCol: range.end.col,
    endRow: range.end.row,
  });
}

function normalizePasteValues(
  tableInstance: VTable.ListTable,
  operationRecordUtils: PasteOperationRecordUtils | undefined,
  col: number,
  row: number,
  values: (string | number)[][],
) {
  return normalizeCreateRowPasteValues({
    values,
    startCol: col,
    startRow: row,
    columns: tableInstance.columns,
    getRowId: (targetRow, targetCol) => tableInstance.getRecordByCell(targetCol, targetRow)?.CHAT2DB_ROW_NUMBER,
    isCreateRow: operationRecordUtils?.isCreateRow,
  });
}

// Existing null cells are pasted as empty strings by the current clipboard format.
const handlePaste = async (tableInstance: VTable.ListTable, operationRecordUtils?: PasteOperationRecordUtils) => {
  if (tableInstance.editorManager?.editingEditor || !tableInstance.stateManager.select.ranges?.length) {
    return;
  }

  const { col, row } = getSelectedPasteRange(tableInstance, 0);
  const pastedData = await readClipboard();
  const values = pastedData.split('\n').map((rowCells) => {
    const cells = rowCells.split('\t');
    return cells.map((cell, cellIndex) => (cellIndex === cells.length - 1 ? cell.trim() : cell));
  });
  const normalizedValues = normalizePasteValues(tableInstance, operationRecordUtils, col, row, values);
  tableInstance.changeCellValues(col, row, normalizedValues);
};

export default handlePaste;
