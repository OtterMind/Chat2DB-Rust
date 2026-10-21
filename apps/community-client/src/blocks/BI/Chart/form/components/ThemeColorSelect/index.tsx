/*
 *
 * ThemeColorSelect
 * report theme color selector
 */
import { memo } from 'react';
import { useStyles } from './style';
import { Select, type SelectProps } from 'antd';
import { CHART_COLORS, CHART_COLORS_MAP } from '@/blocks/BI/Chart/constants';
type LabelRender = SelectProps['labelRender'];

interface IProps {
  value: any;
  onChange?: (value: any) => void;
}

const ColorStripe = ({ colors }) => {
  const { styles } = useStyles();
  return (
    <div className={styles.container}>
      <div className={styles.colorStripe}>
        {colors.map((color) => {
          return <div key={color} className={styles.colorStripeItem} style={{ backgroundColor: color }} />;
        })}
      </div>
    </div>
  );
};

const ThemeColorSelect = (props: IProps) => {
  const { value, onChange } = props;

  const labelRender: LabelRender = (params) => {
    return <ColorStripe colors={CHART_COLORS_MAP[params.value]} />;
  };

  return (
    <Select
      value={value}
      style={{ width: '100%' }}
      labelRender={labelRender}
      optionRender={(option) => {
        return <ColorStripe colors={option.data.colors} />;
      }}
      options={CHART_COLORS}
      onChange={onChange}
    />
  );
};

export default memo(ThemeColorSelect);
