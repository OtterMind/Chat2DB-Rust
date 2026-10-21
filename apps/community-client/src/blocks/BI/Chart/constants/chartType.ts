import i18n from '@/i18n';
import { ChartType } from '.';

export const chartTypeMap = {
  [ChartType.Column]: {
    title: i18n('dashboard.chart.column'),
    icon: 'icon-bar-chart',
    type: ChartType.Column,
  },
  [ChartType.Bar]: {
    title: i18n('dashboard.chart.bar'),
    icon: 'icon-chart-bar-alt',
    type: ChartType.Bar,
  },
  [ChartType.Line]: {
    title: i18n('dashboard.chart.line'),
    icon: 'icon-line-chart',
    type: ChartType.Line,
  },
  [ChartType.AreaLine]: {
    title: i18n('dashboard.chart.areaLine'),
    icon: 'icon-area-line-chart',
    type: ChartType.AreaLine,
  },
  [ChartType.SmoothLine]: {
    title: i18n('dashboard.chart.smoothLine'),
    icon: 'icon-line-chart',
    type: ChartType.SmoothLine,
  },
  [ChartType.StepLine]: {
    title: i18n('dashboard.chart.stepLine'),
    icon: 'icon-line-chart',
    type: ChartType.StepLine,
  },
  [ChartType.Scatter]: {
    title: i18n('dashboard.chart.scatter'),
    icon: 'icon-scatter-chart',
    type: ChartType.Scatter,
  },
  [ChartType.Pie]: {
    title: i18n('dashboard.chart.pie'),
    icon: 'icon-pie-chart',
    type: ChartType.Pie,
  },
  [ChartType.RingPie]: {
    title: i18n('dashboard.chart.ringPie'),
    icon: 'icon-pie-chart',
    type: ChartType.RingPie,
  },
  [ChartType.RosePie]: {
    title: i18n('dashboard.chart.rosePie'),
    icon: 'icon-pie-chart',
    type: ChartType.RosePie,
  },
  [ChartType.Funnel]: {
    title: i18n('dashboard.chart.funnel'),
    icon: 'icon-funnel-chart',
    type: ChartType.Funnel,
  },
  [ChartType.WordCloud]: {
    title: i18n('dashboard.chart.wordCloud'),
    icon: 'icon-wordcloud-chart',
    type: ChartType.WordCloud,
  },
  [ChartType.Statistics]: {
    title: i18n('dashboard.chart.statistics'),
    icon: 'icon-value-chart',
    type: ChartType.Statistics,
  },
  [ChartType.Combo]: {
    title: i18n('dashboard.chart.combo'),
    icon: 'icon-combo-chart',
    type: ChartType.Combo,
  },
  [ChartType.Table]: {
    title: i18n('dashboard.chart.table'),
    icon: 'icon-table',
    type: ChartType.Table,
  },
};

export const chartTypeConfig = [
  {
    // bar chart
    title: i18n('dashboard.chart.bar'),
    components: [chartTypeMap[ChartType.Column], chartTypeMap[ChartType.Bar]],
  },
  {
    // line chart
    title: i18n('dashboard.chart.line'),
    components: [
      chartTypeMap[ChartType.Line],
      chartTypeMap[ChartType.AreaLine],
    ],
  },
  {
    // combination diagram
    title: i18n('dashboard.chart.combo'),
    components: [chartTypeMap[ChartType.Combo]],
  },
  {
    // pie chart
    title: i18n('dashboard.chart.pie'),
    components: [chartTypeMap[ChartType.Pie], chartTypeMap[ChartType.RingPie], chartTypeMap[ChartType.RosePie]],
  },
  {
    // scatter plot
    title: i18n('dashboard.chart.scatter'),
    components: [chartTypeMap[ChartType.Scatter]],
  },
  {
    // Others
    title: i18n('common.text.rests'),
    components: [
      chartTypeMap[ChartType.Table],
      chartTypeMap[ChartType.Funnel],
      chartTypeMap[ChartType.WordCloud],
      chartTypeMap[ChartType.Statistics],
    ],
  },
];
