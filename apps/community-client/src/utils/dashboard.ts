import { TableDataType } from '@/constants/table';
import { DragLayoutItem } from '@/typings/dashboard';
import { IManageResultData } from '@/typings/database';

const getCellDisplayValue = (cell) => {
  if (cell && typeof cell === 'object' && 'value' in cell) {
    return cell.value;
  }
  return cell;
};

export const newFormattedSqlExecuteData = (dataItem?: IManageResultData) => {
  if (!dataItem) {
    return [];
  }
  const dataItemDataList = dataItem.dataList;
  const headerList = dataItem.headerList;
  const data =
    dataItemDataList?.map((item) => {
      const obj = {};
      headerList?.forEach((header, i) => {
        // The first column is the serial number.
        // if (i === 0) return;
        if (header.dataType.toLowerCase() === 'chat2db_row_number') return;
        const headerName = header.name;
        obj[headerName] = getCellDisplayValue(item[i]);
        if (header.dataType === TableDataType.NUMERIC) {
          // Convert obj[headerName] to Number type
          obj[headerName] = Number(obj[headerName]);
          // If there are more than 4 decimal places after the decimal point, keep 4 digits
          // if (obj[headerName] && obj[headerName].toString().split('.')[1]?.length > 4) {
          //   obj[headerName] = Number(obj[headerName].toFixed(4));
          // }
        }
      });
      return obj;
    }) || [];
  return data;
};

// Reverse newFormattedSqlExecuteData converts data in the form of key:value into headerList and dataList
export const reverseFormattedSqlExecuteData = (data: Array<{ [key: string]: any }>) => {
  if (!data || data.length === 0) {
    return;
  }

  const headerList = Object.keys(data[0]).map((key) => ({
    name: key,
    dataType: typeof data[0][key] === 'number' ? TableDataType.NUMERIC : 'string', // It is assumed here that there are only two types: numbers and strings
  }));

  const dataList = data.map((item) => headerList.map((header) => item[header.name]));

  return {
    headerList,
    dataList,
  };
};

// Extract data from the schema in dashboardItem
export const extractDataFromSchema = (schema?: string): any[] | null => {
  if (!schema) {
    return null;
  }
  try {
    const { data } = JSON.parse(schema);
    return data;
  } catch {
    return null;
  }
};

// Determine whether the schema is legal. Regularly determine whether it starts with [{
export const deconstructSchema = (schema?: string): DragLayoutItem[] | null => {
  let result = null;
  if (!schema) {
    return result;
  }
  // Detection point 1
  const check1 = schema.startsWith('[{');
  if (!check1) {
    return result;
  }
  try {
    result = JSON.parse(schema);
  } catch {
    result = null;
  }
  return result;
};

// Given a chartIdList and schema, if there is a chartId in the schema that does not belong to chartIdList, delete it.
export const filterSchemaByChartIds = (chartIds?: number[], schema?: string) => {
  if (!chartIds || !schema) {
    return '';
  }
  const layout = deconstructSchema(schema);
  if (!layout) {
    return '';
  }
  const newLayout = layout.filter((item) => chartIds.includes(Number(item.i)));
  return JSON.stringify(newLayout);
};

// Initialize a layoutItem
export const initLayoutItem = (t, index, obg?): DragLayoutItem => {
  return {
    i: t,
    x: (index % 2) * 6,
    y: Math.floor(index / 2),
    w: 6,
    h: 4,
    minW: 3,
    minH: 3,
    maxH: 10,
    ...(obg || {}),
  };
};

// Initialize a layout
export const initChartIdsLayout = (chartIds: number[]) => {
  return chartIds.map((t, index) => {
    return initLayoutItem(t, index);
  });
};

// Initialize a layout
export const initAppendLayoutItems = (
  chartIds:
    | number[]
    | {
        id: string;
        detail: any;
      }[],
  schema,
) => {
  // Find the maximum y in the schema
  let maxY = 0;
  const layout = deconstructSchema(schema);
  if (layout) {
    layout.forEach((item) => {
      if (item.y > maxY) {
        maxY = item.y;
      }
    });
  }

  // if not an array of objects
  if (typeof chartIds[0] === 'number') {
    return chartIds.map((t, index) => {
      return initLayoutItem(t, index, {
        x: (index % 2) * 6,
        y: Math.floor((index + maxY * 2) / 2) + 1,
      });
    });
  } else {
    return chartIds.map((t, index) => {
      return initLayoutItem(t.id, index, {
        x: (index % 2) * 6,
        y: Math.floor((index + maxY * 2) / 2) + 1,
        detail: t.detail,
      });
    });
  }
};

// Add a chartId to the schema
export const appendLayoutItems = (
  chartIds: number[],
  originalData?: {
    chartIds: number[];
    schema: string;
  },
) => {
  const { schema, chartIds: originalChartIds } = originalData || { schema: '', chartIds: [] };

  let layout = deconstructSchema(schema);

  // Compatible with old data, there may be cases where the schema is empty, but the chartIds have values.
  if (!layout && originalChartIds.length) {
    layout = initChartIdsLayout(originalChartIds);
  }

  // New layout added
  const newNotInLayout = initAppendLayoutItems(chartIds, schema);

  if (!layout?.length) {
    layout = newNotInLayout;
  } else {
    layout = layout.concat(newNotInLayout);
  }

  return JSON.stringify(layout);
};

// Update a value in the schema
export const updateSchemaValue = (value: Record<string, any>, schema?: string) => {
  if (!schema) {
    return JSON.stringify(value);
  }
  let schemaObj: any = null;
  try {
    schemaObj = JSON.parse(schema);
  } catch {
    return schema;
  }

  return JSON.stringify({
    ...schemaObj,
    ...value,
  });
};
