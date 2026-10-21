import i18n from '@/i18n';
import {orderByRuleConfig} from './baseConfig';

export const barConfig = {
  properties: [
    {
      type: 'custom',
      title: i18n('dashboard.chart.xField'),
      name: 'xField',
      component: 'AxisSelect',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.yField'),
      name: 'yField',
      component: 'AxisSelect',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.chartOption'),
      name: 'chartOptionCheckbox',
      component: 'ChartOptionCheckbox',
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
