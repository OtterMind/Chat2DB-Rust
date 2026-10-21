import { memo, useMemo } from 'react';
import EChartsContainer from '@/blocks/BI/Chart/components/EChartsContainer';
import { ChartType, LineType } from '../../constants';
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
  const { chartType, lineType } = chartSchema;

  const { colorBorder } = theme;

  const { xAxis, yAxis, seriesData, seriesName } = useMemo(() => {
    return barDataTreating({ data, chartSchema, direction });
  }, [data, chartSchema, direction]);

  // area chart option
  const areaOption = useMemo(() => {
    if (chartType === ChartType.AreaLine) {
      return {
        areaStyle: {},
        boundaryGap: false,
      };
    }
    return {};
  }, [chartType]);

  // SmoothLine option
  const smoothOption = useMemo(() => {
    if (chartType === ChartType.SmoothLine || lineType === LineType.Smooth) {
      return {
        smooth: true,
      };
    }
    return {};
  }, [chartType]);

  // stepLine
  const stepOption = useMemo(() => {
    if (chartType === ChartType.StepLine || lineType === LineType.Step) {
      return {
        step: 'end',
        boundaryGap: false,
      };
    }
    return {};
  }, [chartType]);

  const option = useMemo(() => {
    let _option = {
      tooltip: {
        trigger: 'axis',
      },
      xAxis: {
        ...xAxis,
        splitLine: {
          lineStyle: {
            color: colorBorder, // Set grid line color
          },
        },
        ...areaOption,
        ...stepOption,
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
        top: '3%', // grid is 3% from the top of the container
        left: '4%', // grid is 3% from the left side of the container
        right: '4%', // grid is 4% away from the right side of the container
        bottom: '5%', // grid is 3% from the bottom of the container
        containLabel: true, // ensures that axis labels are included in the grid
      },
      series: [
        {
          name: seriesName,
          type: 'line',
          data: seriesData,
          smooth: false,
          roseType: 'radius',
          label: {
            show: true,
            position: 'top',
          },
          ...areaOption,
          ...smoothOption,
          ...stepOption,
        },
      ],
    };
    if (chartSchema) {
      _option = handleOptions(chartSchema, _option);
    }
    return _option;
  }, [chartSchema, colorBorder, chartType, seriesData, seriesName, xAxis, yAxis, areaOption, smoothOption, stepOption]);

  return <EChartsContainer option={option} chartSchema={chartSchema} />;
};

export default memo(BarChart);
