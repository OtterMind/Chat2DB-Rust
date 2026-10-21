import { memo, useMemo } from 'react';
import EChartsContainer from '@/blocks/BI/Chart/components/EChartsContainer';
import { ChartType } from '../../constants';
import { useStyles } from './style';
import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';
import handleOptions from '@/blocks/BI/Chart/chartOptionUtils/handleOption';
import { barDataTreating } from './dataTreating';

export interface BarChartProps {
  data?: INormalizedData;
  direction?: 'vertical' | 'horizontal';
  chartSchema: ChartSchema;
}

const BarChart = (props: BarChartProps) => {
  const { theme } = useStyles();
  const { data, chartSchema, direction = 'vertical' } = props;
  const { chartType } = chartSchema;

  const { colorBorder } = theme;

  const seriesChartType = useMemo(() => {
    if (!chartType) {
      return 'bar';
    }
    if (chartType === ChartType.Scatter) {
      return 'scatter';
    }
    return [ChartType.Line, ChartType.AreaLine, ChartType.SmoothLine, ChartType.StepLine].includes(chartType)
      ? 'line'
      : 'bar';
  }, [chartType]);

  const barOptionConfig = useMemo(() => {
    if (chartType === ChartType.Bar) {
      return {
        label: {
          show: true,
          position: 'right',
        },
      };
    }
    return {};
  }, [chartType]);

  const { xAxis, yAxis, seriesData, seriesName } = useMemo(() => {
    return barDataTreating({ data, chartSchema, direction });
  }, [data, chartSchema, direction]);

  const option = useMemo(() => {
    let _option = {
      tooltip: {
        trigger: 'item',
      },
      xAxis: {
        ...xAxis,
        splitLine: {
          lineStyle: {
            color: colorBorder, // Set grid line color
          },
        },
      },
      yAxis: {
        ...yAxis,
        splitLine: {
          lineStyle: {
            color: colorBorder, // Set grid line color
          },
        },
      },
      grid: {
        top: '3%', // grid is 10% from the top of the container
        left: '4%', // grid is 4% from the left side of the container
        right: '4%', // grid is 4% away from the right side of the container
        bottom: '5%', // grid is 5% from the bottom of the container
        containLabel: true, // ensures that axis labels are included in the grid
      },
      series: [
        {
          name: seriesName,
          type: seriesChartType,
          data: seriesData,
          roseType: 'radius',
          animationType: 'scale',
          animationEasing: 'elasticOut',
          label: {
            show: true,
            position: 'top',
          },
          ...barOptionConfig,
        },
      ],
    };
    if (chartSchema) {
      _option = handleOptions(chartSchema, _option);
    }
    return _option;
  }, [chartSchema, colorBorder, seriesChartType, chartType, seriesData, seriesName, xAxis, yAxis]);

  return <EChartsContainer option={option} chartSchema={chartSchema} />;
};

export default memo(BarChart);
