import i18n from '@/i18n';
import { ChartType } from '../../constants';

export const funnelConfig = {
  properties: [
    {
      type: 'custom',
      title: i18n('dashboard.chart.groupField.funnel'),
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
        chartType: ChartType.Funnel,
      }
    },
    // {
    //   type: 'custom',
    //   title: i18n('dashboard.chart.chartOption'),
    //   name: 'chartOptionCheckbox',
    //   component: 'ChartOptionCheckbox',
    //   componentProps: {
    //     chartType: ChartType.Pie,
    //   }
    // },
    // {
    //   type: 'custom',
    //   title: i18n('dashboard.chart.orderByType'),
    //   name: 'orderByType',
    //   component: 'OrderByTypeSelect',
    // },
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
