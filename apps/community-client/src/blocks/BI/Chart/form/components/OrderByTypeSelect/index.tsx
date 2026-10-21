/*
 * ThemeColorSelect
 * report theme color selector
 */
import { memo } from 'react';
import { Radio } from 'antd';
import { OrderByType } from '@/blocks/BI/Chart/constants';
import i18n from '@/i18n';

interface IProps {
  value: any;
  onChange?: (value: any) => void;
}

const OrderByTypeSelect = (props: IProps) => {
  const { value, onChange } = props;
  return (
    <Radio.Group value={value} onChange={onChange}>
      <Radio value={OrderByType.DEFAULT}>{i18n('dashboard.chart.orderByType.data')}</Radio>
      <Radio value={OrderByType.X_AXIS}>{i18n('dashboard.chart.orderByType.xAxis')}</Radio>
      <Radio value={OrderByType.Y_AXIS}>{i18n('dashboard.chart.orderByType.yAxis')}</Radio>
    </Radio.Group>
  );
};

export default memo(OrderByTypeSelect);
