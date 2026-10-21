import { IManageResultData } from '@/typings';

export const GENERATED_VALUE = 'CHAT2DB_UPDATE_TABLE_DATA_USER_FILLED_GENERATED';
export const DEFAULT_VALUE = 'CHAT2DB_UPDATE_TABLE_DATA_USER_FILLED_DEFAULT';

export interface CreateRowPasteColumn {
  originalData?: IManageResultData['headerList'][number] | null;
}

export interface PasteTargetCell {
  row: number;
  col: number;
}

export interface PasteTargetRange {
  startCol: number;
  startRow: number;
  endCol: number;
  endRow: number;
}

export const getBlankCreateCellValue = (header: IManageResultData['headerList'][number]) => {
  if (header.autoIncrement === 1) {
    return GENERATED_VALUE;
  }
  if (header.defaultValue === null || header.defaultValue === undefined) {
    return null;
  }
  return DEFAULT_VALUE;
};

export const getClonedCreateCellValue = (header: IManageResultData['headerList'][number], value: any) => {
  if (header.autoIncrement === 1) {
    return GENERATED_VALUE;
  }
  return value;
};

export const normalizePasteTargetCell = ({ row, col }: PasteTargetCell) => {
  return {
    row: row === 0 ? 1 : row,
    col: col === 0 ? 1 : col,
  };
};

export const normalizePasteTargetRange = ({ startCol, startRow, endCol, endRow }: PasteTargetRange) => {
  const start = normalizePasteTargetCell({
    col: Math.min(startCol, endCol),
    row: Math.min(startRow, endRow),
  });
  const end = normalizePasteTargetCell({
    col: Math.max(startCol, endCol),
    row: Math.max(startRow, endRow),
  });
  return {
    col: start.col,
    row: start.row,
    maxCol: Math.max(start.col, end.col),
    maxRow: Math.max(start.row, end.row),
  };
};

export const normalizeCreateRowPasteValues = ({
  values,
  startCol,
  startRow,
  columns,
  getRowId,
  isCreateRow,
}: {
  values: any[][];
  startCol: number;
  startRow: number;
  columns?: CreateRowPasteColumn[];
  getRowId: (row: number, col: number) => string | number | null | undefined;
  isCreateRow?: (rowId?: string | number | null) => boolean;
}) => {
  if (!isCreateRow) {
    return values;
  }

  return values.map((rowValues, rowIndex) => {
    return rowValues.map((value, colIndex) => {
      const targetRow = startRow + rowIndex;
      const targetCol = startCol + colIndex;
      const rowId = getRowId(targetRow, targetCol);
      if (!isCreateRow(rowId)) {
        return value;
      }

      const header = columns?.[targetCol - 1]?.originalData;
      if (!header) {
        return value;
      }
      return getClonedCreateCellValue(header, value);
    });
  });
};

// Convert the row data object obtained from the table into an array according to headerList
// The first parameter is object, the second parameter is headerList
export const transformRowData = (
  rowData: { [key: string]: any },
  headerList: IManageResultData['headerList'],
  rowNumber?: string | number,
) => {
  const list: any = [];
  headerList.forEach((item, index) => {
    if (index === 0) {
      if (rowNumber !== undefined) {
        list.push(rowNumber.toString());
      }
      return;
    }

    list.push(rowData[index.toString()]);
  });
  return list;
};

export const transformOperations = (operations, headerList: IManageResultData['headerList']) => {
  return operations.map((operation) => {
    const res = {
      ...operation,
    };
    if (res.dataList) {
      res.dataList = transformRowData(operation.dataList, headerList, operation.rowId);
    }
    if (res.selectCols?.length) {
      res.selectCols = res.selectCols.map((field) => {
        return headerList.findIndex((item, index) => index.toString() === field);
      });

      // excludes -1. CHAT2DB_ROW_NUMBER
      res.selectCols = res.selectCols.filter((index) => index !== -1);
    }
    if (res.oldDataList) {
      res.oldDataList = transformRowData(operation.oldDataList, headerList, operation.rowId);
    }
    return res;
  });
};
