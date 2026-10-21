import React, { memo } from 'react';
import { ChartType } from '@/constants/dashboard';
import BarChart from '@/blocks/BI/Chart/charts/BarChart';
import LineChart from '@/blocks/BI/Chart/charts/LineChart';
import PieChart from '@/blocks/BI/Chart/charts/PieChart';
import { DivProps } from '@/typings/common';

export interface ChartProps extends DivProps {
  className?: string;
  chartType?: ChartType;
  emptyComment?: React.ReactNode;
  chartConfig?: any;
}

const Chart = (props: ChartProps) => {
  const { chartType } = props;

  const dispatcher = (type?: ChartType) => {
    switch (type) {
      case ChartType.Bar:
      case ChartType.Column:
        return <BarChart />;
      case ChartType.Line:
        return <LineChart />;
      case ChartType.Pie:
        return <PieChart />;
      default:
        return null;
    }
  };

  return dispatcher(chartType);
};

export default memo(Chart);
