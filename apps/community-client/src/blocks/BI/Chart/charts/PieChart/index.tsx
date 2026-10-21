import { memo, useMemo } from 'react';
import EChartsContainer from '@/blocks/BI/Chart/components/EChartsContainer';
import { useStyles } from './style';
import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';
import { pieDataTreating } from './dataTreating';
import handleOptions from '../../chartOptionUtils/handleOption';
import { ChartType } from '../../constants';
import _ from 'lodash';

export interface BarChartProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
}

const PieChart = (props: BarChartProps) => {
  const { theme } = useStyles();

  const { data, chartSchema } = props;
  const { chartType } = chartSchema;

  const { seriesData } = pieDataTreating({ data, chartSchema });

  const { colorBgContainer, colorTextSecondary } = theme;

  // rose chart option
  const roseOption = useMemo(() => {
    if (chartType === ChartType.RosePie) {
      return {
        radius: [50, '70%'],
        roseType: 'area',
        itemStyle: {
          borderRadius: 8,
          borderColor: colorBgContainer,
          borderWidth: 2,
        },
      };
    }
    return {};
  }, [chartType]);

  // ring chart option
  const ringOption = useMemo(() => {
    if (chartType === ChartType.RingPie) {
      return {
        radius: ['34%', '70%'],
        itemStyle: {
          borderRadius: 6,
          borderColor: colorBgContainer,
          borderWidth: 2,
        },
        emphasis: {
          label: {
            show: true,
            fontSize: 20,
            fontWeight: 'bold',
          },
        },
      };
    }
    return {};
  }, [chartType]);

  const option = useMemo(() => {
    let _option = {
      tooltip: {
        trigger: 'item',
      },
      series: [
        {
          // name: 'Access From', // variable
          type: 'pie',
          center: ['50%', '54%'], // The center (circle center) coordinates of the pie chart
          label: {
            // tag configuration
            formatter: '{b}:{@value} ({d}%)', // Custom label format
            color: colorTextSecondary, // text color
          },
          itemStyle: {
            borderRadius: 0,
            borderColor: colorBgContainer,
            borderWidth: 2,
          },
          data: seriesData,
          animationType: 'scale',
          animationEasing: 'elasticOut',
          emphasis: {
            itemStyle: {
              // shadowBlur: 10,
              shadowOffsetX: 0,
              // shadowColor: 'rgba(0, 0, 0, 0.5)',
            },
          },
          ...roseOption,
          ...ringOption,
        },
      ],
    };
    if (chartSchema) {
      _option = handleOptions(chartSchema, _option);
      // Delete xAxis and yAxis
      _.unset(_option, 'xAxis');
      _.unset(_option, 'yAxis');
    }
    return _option;
  }, [seriesData]);

  return <EChartsContainer option={option} chartSchema={chartSchema} />;
};

export default memo(PieChart);
