import i18n from '@/i18n';
import { ChartType } from '../../constants';
import {orderByRuleConfig} from './baseConfig';

export const pieConfig = {
  properties: [
    {
      type: 'custom',
      title: i18n('dashboard.chart.groupField.pie'),
      name: 'xField',
      component: 'AxisSelect',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.angleField'),
      name: 'yField',
      component: 'AxisSelect',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.chartOption'),
      name: 'chartOptionCheckbox',
      component: 'ChartOptionCheckbox',
      componentProps: {
        chartType: ChartType.Pie,
      }
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.orderByType'),
      name: 'orderByType',
      component: 'OrderByTypeSelect',
    },
    orderByRuleConfig,
    {
      type: 'custom',
      title: i18n('dashboard.chart.themeColor'),
      name: 'themeColorCode',
      component: 'ThemeColorSelect',
    },
    // {
    //   type: 'custom',
    //   name: 'autoRefresh',
    //   component: 'AutoRefreshSelect',
    // },
  ],
};
