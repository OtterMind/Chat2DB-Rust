import { memo, useMemo } from 'react';
import EChartsContainer from '@/blocks/BI/Chart/components/EChartsContainer';
import { useStyles } from './style';
import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';
import { pieDataTreating } from './dataTreating';
import handleOptions from '../../chartOptionUtils/handleOption';
import _ from 'lodash';

export interface BarChartProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
}

const FunnelChart = (props: BarChartProps) => {
  const { theme } = useStyles();

  const { data, chartSchema } = props;
  const { seriesData } = pieDataTreating({ data, chartSchema });

  const { colorBgContainer, colorTextSecondary } = theme;

  const option = useMemo(() => {
    let _option = {
      tooltip: {
        trigger: 'item',
      },
      series: [
        {
          name: 'Access From', // variable
          type: 'funnel',
          left: '10%',
          top: 60,
          bottom: 60,
          width: '80%',
          min: 0,
          max: 100,
          minSize: '0%',
          maxSize: '100%',
          sort: 'descending',
          gap: 2,
          itemStyle: {
            borderColor: colorBgContainer,
            borderWidth: 1,
          },
          // label: {
          //   // formatter: '{b}:{@value} ({d}%)', // Custom label format
          //   formatter: '{@value} ({d}%)', // Custom label format
          //   color: colorTextSecondary, // text color
          // },
          label: {
            show: true,
            position: 'inside',
            color: colorTextSecondary, // text color
          },
          labelLine: {
            length: 10,
            lineStyle: {
              width: 1,
              type: 'solid',
            },
          },
          emphasis: {
            label: {
              fontSize: 20,
            },
          },
          data: seriesData,
          animationType: 'scale',
          animationEasing: 'elasticOut',
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

export default memo(FunnelChart);
