import { memo } from 'react';
import { Segmented } from 'antd';
import { LineType } from '@/blocks/BI/Chart/constants';
import i18n from '@/i18n';

interface IProps {
  value: LineType;
  onChange?: (value: LineType) => void;
}

const LineTypeSelect = (props: IProps) => {
  const { value, onChange } = props;

  const options = [
    {
      value: LineType.Straight,
      label: i18n('dashboard.chart.linear'),
    },
    {
      value: LineType.Smooth,
      label: i18n('dashboard.chart.smooth'),
    },
    {
      value: LineType.Step,
      label: i18n('dashboard.chart.step'),
    },
  ];

  return <Segmented value={value} onChange={onChange} options={options} block />;
};

export default memo(LineTypeSelect);
