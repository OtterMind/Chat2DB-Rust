import i18n from '@/i18n';

export const statisticsConfig = {
  properties: [
    {
      type: 'custom',
      title: i18n('dashboard.chart.angleField'),
      name: 'xField',
      component: 'AxisSelect',
    },
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
