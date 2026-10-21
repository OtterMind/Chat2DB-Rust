/*
 * ThemeColorSelect
 * report theme color selector
 */
import { memo, useMemo } from 'react';
import { Select } from 'antd';
import { IChartItem } from '@/typings';
import { newFormattedSqlExecuteData } from '@/utils/dashboard';

interface IProps {
  value: any;
  onChange?: (value: any) => void;
  chartDetail: IChartItem;
}

const AxisSelect = (props: IProps) => {
  const { value, onChange, chartDetail } = props;

  const dataKeys = useMemo(() => {
    if (!chartDetail.metaData) {
      return [];
    }
    const metaData = newFormattedSqlExecuteData(chartDetail.metaData);
    const keys = Object.keys(metaData[0] || {});
    const data =
      keys?.map((key) => {
        return { value: key, label: key };
      }) || [];
    return data;
  }, [chartDetail.metaData]);

  return <Select value={value} onChange={onChange} options={dataKeys} />;
};

export default memo(AxisSelect);
