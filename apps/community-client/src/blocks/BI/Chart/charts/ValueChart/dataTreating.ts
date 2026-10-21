import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';

export interface DataTreatingProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
}

export const pieDataTreating = (props: DataTreatingProps) => {
  const { data, chartSchema } = props;

  const { xField, valueField } = chartSchema;

  let _valueField = valueField || xField;
  // dynamically searches for keys in data
  if (data && data.length > 0) {
    _valueField = Object.keys(data[0]).find((key) => key.toLowerCase() === _valueField?.toLowerCase()) || _valueField;
  }

  const _xField = _valueField || 'name';

  const seriesData = data?.[0]?.[_xField] || 0;

  return {
    seriesData: seriesData.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ','),
  };
};
