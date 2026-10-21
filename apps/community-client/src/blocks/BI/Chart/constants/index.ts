// chart theme color
export const CHART_COLORS = [
  {
    // light blue [4]
    label: '淡蓝色系',
    value: 'v1-baby-blue',
    colors: ['#1f88e6', '#2196f3', '#42a5f5', '#64b5f5', '#90caf8', '#bbdffc'],
  },
  // [1]
  {
    label: '淡粉色系', 
    value: 'v1-baby-pink',
    colors: ['#d7c2ea', '#e7d3ee', '#c5a7c5', '#f9c5d2', '#b2cf98', '#eeeaeb'],
  },
  // [2]
  {
    label: '淡紫色系',
    value: 'v1-baby-purple',
    colors: ['#feb1d9', '#d695e1', '#ad8fdb', '#8477c6', '#86ddf4', '#72bcec'],
  },
  // [3]
  {
    label: '淡米色系',
    value: 'v1-baby-beige',
    colors: ['#f9f3d3', '#fad6b5', '#fcafae', '#fedfe5', '#ddf0ee', '#b6e3e8']
  },
  // [5]
  {
    label: '淡棕色系',
    value: 'v1-baby-brown',
    colors: ['#efcfaf', '#efae6b', '#e4aa72', '#dd936f', '#b9744e', '#ba6830']
  },
  // [6]
  {
    label: '淡黄色系',
    value: 'v1-baby-yellow',
    colors: ['#ffa001', '#ffb400', '#ffc107', '#ffca27', '#ffd550', '#fee180'],
  },
  // [7]
  {
    label: '淡绿色系',
    value: 'v1-baby-green',
    colors: ['#679f38', '#7db343', '#8bc349', '#9ace63', '#afd582', '#c5e1a5']
  },
  // [8]
  {
    label: '淡橙色系',
    value: 'v1-baby-orange',
    colors: ['#f67d01', '#fb8c00', '#ff9800', '#ffa725', '#feb74d', '#ffcc80']
  },
  // [9]
  {
    label: '复古色系',
    value: 'v1-retro',
    colors: ['#124faa', '#fec635', '#ef4401', '#198a84', '#f9f2e2', '#e0e0e0'],
  },
  // [11]
  {
    label: '淡灰色系',
    value: 'v1-baby-gray',
    colors: ['#466979', '#79919d', '#93acbb', '#b9c9e1', '#98abbe', '#edf2f8'],
  },
  // Colorful-1
  {
    label: '彩色的',
    value: 'v1-colorful-1',
    colors: ['#1f88e6', '#d7c2ea', '#feb1d9', '#f9f3d3', '#efcfaf', '#ffa001', '#679f38', '#f67d01', '#124faa', '#466979'],
  },
];

export enum ChartType {
  // line chart
  Line = 'Line',
  // area chart
  AreaLine = 'AreaLine',
  // smooth line chart
  SmoothLine = 'SmoothLine',
  // ladder line chart
  StepLine = 'StepLine',
  // bar chart
  Column = 'Column',
  // Bar Chart
  Bar = 'Bar',
  // pie chart
  Pie = 'Pie',
  // Rose Pie Chart
  RosePie = 'RosePie',
  // ring chart
  RingPie = 'RingPie',
  // funnel chart
  Funnel = 'Funnel',
  // word cloud
  WordCloud = 'WordCloud',
  // waterfall chart
  Waterfall = 'Waterfall',
  // histogram
  Histogram = 'Histogram',
  // form
  Table = 'Table',
  // digital
  Statistics = 'Statistics',
  // scatter plot
  Scatter = 'Scatter',
  // combination diagram
  Combo = 'Combo',
}

export const CHART_COLORS_MAP = CHART_COLORS.reduce((acc, cur) => { 
  acc[cur.value] = cur.colors;
  return acc;
}, {});

// AUTO_REFRESH
export enum AUTO_REFRESH { 
  NEVER = 'NEVER',
  MINUTES = 'MINUTES',
  EVERYDAY = 'EVERYDAY',
  CRON = 'CRON',
}

//  Auto refresh
export const AUTO_REFRESH_OPTIONS = [
  // never refreshes
  { label: 'dashboard.refresh.never', value: AUTO_REFRESH.NEVER },
  // refresh every minute
  { label: 'dashboard.refresh.minutes.refresh', value: AUTO_REFRESH.MINUTES },
  // refreshes regularly every day
  { label: 'dashboard.refresh.everyday.refresh', value: AUTO_REFRESH.EVERYDAY },
  // // cron expression
  // { label: 'dashboard.refresh.cron', value: AUTO_REFRESH.CRON },
]

export const AUTO_REFRESH_DASHBOARD_OPTIONS = [
  // never refreshes
  { label: 'dashboard.refresh.never.dashboard', value: AUTO_REFRESH.NEVER },
  // refresh every minute
  { label: 'dashboard.refresh.minutes.dashboard', value: AUTO_REFRESH.MINUTES },
  // // cron expression
  // { label: 'dashboard.refresh.cron', value: AUTO_REFRESH.CRON },
]

export enum OrderByType {
  DEFAULT = 'DEFAULT',
  X_AXIS = 'X_AXIS',
  Y_AXIS = 'Y_AXIS',
}

export enum OrderByRule {
  ASC = 'ASC',
  DESC = 'DESC',
}

export enum WordCloudChartShape { 
  CLOUD = 'CLOUD',
}

export enum LineType {
  Straight = 'straight',
  Smooth = 'Smooth',
  Step = 'step',
}
