/*
 * Report type selection
 */
import { memo, useEffect, useMemo, useState } from 'react';
import { useStyles } from './style';
import { Select, type SelectProps } from 'antd';
type LabelRender = SelectProps['labelRender'];
import { IconfontSvg } from '@chat2db/ui';
import { chartTypeMap, chartTypeConfig } from '@/blocks/BI/Chart/constants/chartType';

interface IProps {
  value: any;
  onChange?: (value: any) => void;
  chartTypeConfig?: any;
}

const ChartTypeListRender = ({ onClick, chartTypeConfig: _chartTypeConfig }) => {
  const { styles } = useStyles();

  const handleClick = (type) => {
    onClick(type);
  };

  const chartTypeConfigList = useMemo(() => {
    return _chartTypeConfig || chartTypeConfig;
  }, []);

  return (
    <div className={styles.groupContainer}>
      {chartTypeConfigList.map((group, index) => {
        return (
          <div className={styles.groupItem} key={index}>
            <div className={styles.groupTitle}>{group.title}</div>
            <div className={styles.componentContainer}>
              {group.components.map((component, index_1) => {
                return (
                  <div
                    className={styles.componentItem}
                    onClick={() => {
                      handleClick(component.type);
                    }}
                    key={index_1}
                  >
                    {component.title}
                  </div>
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
};

const ChartTypeSelect = (props: IProps) => {
  const { value, onChange, chartTypeConfig: _chartTypeConfig } = props;
  const [open, setOpen] = useState(false);
  const [selectValue, setSelectValue] = useState(value);
  const { styles } = useStyles();

  useEffect(() => {
    setSelectValue(value);
  }, [value]);

  const labelRender: LabelRender = (labelRenderProps) => {
    const { value: _value } = labelRenderProps;
    const { icon, title } = chartTypeMap[_value] || {};
    return (
      <div className={styles.label}>
        <IconfontSvg code={icon} />
        {title}
      </div>
    );
  };

  const handleClick = (type) => {
    setOpen(false);
    setSelectValue(type);
    onChange?.(type);
  };

  const handleOpenChange = (_open) => {
    setOpen(_open);
  };

  return (
    <Select
      value={selectValue}
      style={{ width: '100%' }}
      open={open}
      labelRender={labelRender}
      dropdownRender={() => <ChartTypeListRender chartTypeConfig={_chartTypeConfig} onClick={handleClick} />}
      onDropdownVisibleChange={handleOpenChange}
    />
  );
};

export default memo(ChartTypeSelect);
