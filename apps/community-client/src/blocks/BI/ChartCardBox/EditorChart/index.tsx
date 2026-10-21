import React, { memo } from 'react';
import { useStyles } from './style';
import Chart from '@/blocks/BI/Chart';
import { IChartItem } from '@/typings/dashboard';
import ChartTypeAndDataForm from '@/blocks/BI/Chart/form/ChartTypeAndDataForm';
import { isEqualMemo } from '@/utils';

interface IProps {
  className?: string;
  customCommitButton?: React.ReactNode;
  chartDetail: IChartItem;
  onChangeChartSchema?: (values: IChartItem['chartSchema']) => void;
}

export default memo<IProps>(
  (props) => {
    const { className, customCommitButton, chartDetail, onChangeChartSchema } = props;
    const { styles, cx } = useStyles();

    return (
      <div className={cx(styles.editChartCard, className)}>
        <div className={styles.left}>
          <Chart chartDetail={chartDetail} />
        </div>
        <div className={styles.right}>
          <div className={styles.formContent}>
            <ChartTypeAndDataForm chartDetail={chartDetail} onChangeChartSchema={onChangeChartSchema} />
          </div>
          <div className={styles.buttonBox}>{customCommitButton}</div>
        </div>
      </div>
    );
  },
  (prev, next) => {
    return isEqualMemo([prev.chartDetail, next.chartDetail], [prev.className, next.className]);
  },
);
