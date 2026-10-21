/*
 * ThemeColorSelect
 * report theme color selector
 */
import { memo } from 'react';
// import { useStyles } from './style';
import { Radio } from 'antd';
import { OrderByRule, OrderByType } from '@/blocks/BI/Chart/constants';
import i18n from '@/i18n';
import { ChartSchema } from '../../../typings';

interface IProps {
  className?: string;
  formData: ChartSchema;
  value: any;
  onChange?: (value: any) => void;
}

const OrderByRuleSelect = (props: IProps) => {
  const { value, onChange, formData } = props;

  if (formData?.orderByType === OrderByType.DEFAULT) {
    return null;
  }

  return (
    <Radio.Group value={value} onChange={onChange}>
      <Radio value={OrderByRule.ASC}>{i18n('dashboard.chart.orderByRule.asc')}</Radio>
      <Radio value={OrderByRule.DESC}>{i18n('dashboard.chart.orderByRule.desc')}</Radio>
    </Radio.Group>
  );
};

export default memo(OrderByRuleSelect);
