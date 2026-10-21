import ThemeColorSelect from '../components/ThemeColorSelect';
import OrderByTypeSelect from '../components/OrderByTypeSelect';
import ChartOptionCheckbox from '../components/ChartOptionCheckbox';
import AutoRefreshSelect from '../components/RefreshRule';
import OrderByRuleSelect from '../components/OrderByRuleSelect';
import ChartTypeSelect from '../components/ChartTypeSelect';
import AxisSelect from '../components/AxisSelect';
import ComboAxisSelect from '../components/ComboAxisSelect';
import LineTypeSelect from '../components/LineTypeSelect';
import BespreadWordCloud from '../components/BespreadWordCloud';
import {Checkbox} from 'antd';
import i18n from '@/i18n';
import { OrderByType } from '../../constants';

export const componentMap = {
  ChartTypeSelect,
  ThemeColorSelect,
  OrderByTypeSelect,
  ChartOptionCheckbox,
  AutoRefreshSelect,
  OrderByRuleSelect,
  Checkbox,
  AxisSelect,
  ComboAxisSelect,
  LineTypeSelect,
  BespreadWordCloud,
};

export const baseConfig = {
  properties: [
    {
      type: 'custom',
      title: i18n('dashboard.title.chartType'),
      name: 'chartType',
      component: 'ChartTypeSelect',
    },
  ],
};

export const orderByRuleConfig = {
    type: 'custom',
    title: i18n('dashboard.chart.orderByRule'),
    name: 'orderByRule',
    component: 'OrderByRuleSelect',
    hidden: (props) => {
      return props?.orderByType === OrderByType.DEFAULT;
    }
}
