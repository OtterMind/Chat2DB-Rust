/*
 * BespreadWordCloud
 * Is the content covered with word clouds?
 */
import { memo } from 'react';
import { Checkbox } from 'antd';
import i18n from '@/i18n';

interface IProps {
  value: any;
  onChange?: (value: any) => void;
}

const BespreadWordCloud = (props: IProps) => {
  const { value, onChange } = props;
  const handleChange = (e) => {
    onChange?.(e.target.checked);
  };
  return (
    <Checkbox checked={value} style={{ lineHeight: '32px' }} onChange={handleChange}>
      {i18n('dashboard.chart.chartOption.bespreadWordCloud')}
    </Checkbox>
  );
};

export default memo(BespreadWordCloud);
