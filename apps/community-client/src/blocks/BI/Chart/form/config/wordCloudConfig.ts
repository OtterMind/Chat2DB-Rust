import i18n from '@/i18n';

export const wordCloudConfig = {
  properties: [
    {
      type: 'custom',
      title: i18n('dashboard.chart.groupField.wordCloud'),
      name: 'xField',
      component: 'AxisSelect',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.angleField.wordCloud'),
      name: 'yField',
      component: 'AxisSelect',
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
      name: 'bespreadWordCloud',
      component: 'BespreadWordCloud',
    },
    {
      type: 'custom',
      title: i18n('dashboard.chart.themeColor'),
      name: 'themeColorCode',
      component: 'ThemeColorSelect',
    },

  ],
};
