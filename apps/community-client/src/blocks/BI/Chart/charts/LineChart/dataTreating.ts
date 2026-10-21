import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';
import { OrderByRule, OrderByType } from '../../constants';

export interface DataTreatingProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
  direction?: 'vertical' | 'horizontal';
}

export const barDataTreating = (props: DataTreatingProps) => {
  const { data, chartSchema, direction } = props;
  const { orderByType, orderByRule } = chartSchema;
  let { xField, yField } = chartSchema;
  // dynamically searches for keys in data
  if (data && data.length > 0) {
    xField = Object.keys(data[0]).find((key) => key.toLowerCase() === xField?.toLowerCase()) || xField;
    yField = Object.keys(data[0]).find((key) => key.toLowerCase() === yField?.toLowerCase()) || yField;
  }

  let xAxisData: (number | string)[] = [];
  let yAxisData: (number | string)[] = [];
  const seriesName = yField;

  let seriesData: any = null;
  let xAxis: any = null;
  let yAxis: any = null;

  if (xField && yField) {
    data?.forEach((item) => {
      xAxisData.push(item[xField]);
      yAxisData.push(item[yField]);
    });
  }

  if (orderByType === OrderByType.X_AXIS || orderByType === OrderByType.Y_AXIS) {
    const isXAxis = orderByType === OrderByType.X_AXIS;
    const dataToSort = isXAxis ? xAxisData : yAxisData;
    const sortedIndices = dataToSort
      .map((_, index) => index)
      .sort((a, b) => {
        const compareResult = dataToSort[a] < dataToSort[b] ? -1 : 1;
        return orderByRule === OrderByRule.ASC ? compareResult : -compareResult;
      });

    xAxisData = sortedIndices.map((index) => xAxisData[index]);
    yAxisData = sortedIndices.map((index) => yAxisData[index]);
  }

  if (direction === 'horizontal') {
    xAxis = {
      type: 'value',
    };
    yAxis = {
      type: 'category',
      data: yAxisData,
    };
    seriesData = xAxisData;
  } else {
    xAxis = {
      type: 'category',
      data: xAxisData,
    };
    yAxis = {
      type: 'value',
    };
    seriesData = yAxisData;
  }

  return {
    xAxis,
    yAxis,
    seriesName,
    seriesData,
  };
};
