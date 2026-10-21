import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';
import { OrderByRule, OrderByType } from '../../constants';

export interface DataTreatingProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
}

export const pieDataTreating = (props: DataTreatingProps) => {
  const { data, chartSchema } = props;
  const { valueField, angleField, orderByType, orderByRule } = chartSchema;
  
  let { xField, yField } = chartSchema;
  // dynamically searches for keys in data
  if (data && data.length > 0) {
    xField = Object.keys(data[0]).find((key) => key.toLowerCase() === xField?.toLowerCase()) || xField;
    yField = Object.keys(data[0]).find((key) => key.toLowerCase() === yField?.toLowerCase()) || yField;
  }

  const _xField = xField || valueField || 'name';
  const _yField = yField || angleField || 'value';

  let seriesData = data?.map((item) => {
    return {
      name: item[_xField],
      value: item[_yField],
    };
  });

  if (orderByType !== OrderByType.DEFAULT) {
    const sortField = orderByType === OrderByType.X_AXIS ? 'name' : 'value';
    seriesData = seriesData?.sort((a, b) => {
      if (orderByRule === OrderByRule.ASC) {
        return a[sortField] < b[sortField] ? -1 : 1;
      } else {
        return a[sortField] > b[sortField] ? -1 : 1;
      }
    });
  }

  return {
    seriesData,
  };
};
