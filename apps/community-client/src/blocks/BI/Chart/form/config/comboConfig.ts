import i18n from '@/i18n';

export const comboConfig = {
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
      name: 'comboYAxisData',
      component: 'ComboAxisSelect',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.lineType'),
      name: 'lineType',
      component: 'LineTypeSelect',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.chartOption'),
      name: 'chartOptionCheckbox',
      component: 'ChartOptionCheckbox',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.themeColor'),
      name: 'themeColorCode',
      component: 'ThemeColorSelect',
    },
  ],
};
