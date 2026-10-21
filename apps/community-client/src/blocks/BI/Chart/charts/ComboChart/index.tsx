import { memo, useMemo } from 'react';
import EChartsContainer from '@/blocks/BI/Chart/components/EChartsContainer';
import { useStyles } from './style';
import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';
import handleOptions from '@/blocks/BI/Chart/chartOptionUtils/handleOption';
import { barDataTreating } from './dataTreating';

export interface BarChartProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
}

const ComboChart = (props: BarChartProps) => {
  const { theme } = useStyles();
  const { data, chartSchema } = props;

  const { colorBorder } = theme;

  const { xAxis, series } = useMemo(() => {
    return barDataTreating({ data, chartSchema });
  }, [data, chartSchema]);

  const option = useMemo(() => {
    let _option = {
      tooltip: {
        trigger: 'axis',
      },
      xAxis: [
        {
          ...xAxis,
          splitLine: {
            lineStyle: {
              color: colorBorder, // Set grid line color
            },
          },
        },
        {
          ...xAxis,
          boundaryGap: false, // The right X axis does not display grid lines
          axisLabel: {
            show: false,
          },
          splitLine: {
            lineStyle: {
              color: colorBorder, // Set grid line color
            },
          },
          axisPointer: {
            show: false,
          },
        },
      ],
      yAxis: [
        {
          type: 'value',
          splitLine: {
            lineStyle: {
              color: colorBorder, // Set grid line color
            },
          },
        },
        {
          type: 'value',
          position: 'right',
          splitLine: {
            show: false, // The right Y axis does not display grid lines
            lineStyle: {
              color: colorBorder, // Set grid line color
            },
          },
        },
      ],
      grid: {
        top: '3%', // grid is 3% from the top of the container
        left: '4%', // grid is 3% from the left side of the container
        right: '4%', // grid is 4% away from the right side of the container
        bottom: '5%', // grid is 3% from the bottom of the container
        containLabel: true, // ensures that axis labels are included in the grid
      },
      series,
    };
    if (chartSchema) {
      _option = handleOptions(chartSchema, _option);
    }
    return _option;
  }, [chartSchema, colorBorder, series, xAxis]);

  return <EChartsContainer option={option} chartSchema={chartSchema} />;
};

export default memo(ComboChart);
