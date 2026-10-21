import { memo, useMemo } from 'react';
import { IChartItem } from '@/typings/dashboard';
import ChartForm from '@/blocks/BI/Chart/form';
import { barConfig } from '../config/barConfig';
import { baseConfig } from '../config/baseConfig';
import { lineConfig } from '../config/lineConfig';
import { tableConfig } from '../config/tableConfig';
import { pieConfig } from '../config/pieConfig';
import { funnelConfig } from '../config/funnelConfig';
import { wordCloudConfig } from '../config/wordCloudConfig';
import { statisticsConfig } from '../config/statisticsConfig';
import { comboConfig } from '../config/comboConfig';
import { ChartType } from '../../constants';

interface IProps {
  className?: string;
  chartDetail?: IChartItem;
  onChangeChartSchema?: (values: IChartItem['chartSchema']) => void;
}

export default memo<IProps>((props) => {
  const { chartDetail, onChangeChartSchema } = props;

  const formConfig = useMemo(() => {
    switch (chartDetail?.chartSchema?.chartType) {
      case ChartType.Bar:
      case ChartType.Column:
      case ChartType.Scatter:
        return barConfig;
      case ChartType.Line:
      case ChartType.AreaLine:
      case ChartType.SmoothLine:
      case ChartType.StepLine:
        return lineConfig;
      case ChartType.Table:
        return tableConfig;
      case ChartType.Pie:
      case ChartType.RingPie:
      case ChartType.RosePie:
        return pieConfig;
      case ChartType.Funnel:
        return funnelConfig;
      case ChartType.WordCloud:
        return wordCloudConfig;
      case ChartType.Statistics:
        return statisticsConfig;
      case ChartType.Combo:
        return comboConfig;
      default:
        return { properties: [] };
    }
  }, [chartDetail?.chartSchema?.chartType]);

  return (
    <ChartForm
      formConfig={[...baseConfig.properties, ...formConfig.properties]}
      chartDetail={chartDetail}
      onChangeChartSchema={onChangeChartSchema}
    />
  );
});
