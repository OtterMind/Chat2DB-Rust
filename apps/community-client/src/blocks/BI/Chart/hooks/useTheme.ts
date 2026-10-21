import { createStyles } from 'antd-style';
import { useMemo } from 'react';
import { CHART_COLORS, CHART_COLORS_MAP } from '../constants';
import { ChartTheme } from '@/typings/dashboard';


export const useChartTheme = (chartTheme?: ChartTheme) => {
  const useStyles = createStyles(() => {
    return {};
  });

  const { theme: antdTheme } = useStyles();

  const { colorPrimary, colorText, colorTextSecondary, colorBgContainer } = antdTheme;

  const color = useMemo(() => {
    if (chartTheme?.themeColorCode) { 
      return CHART_COLORS_MAP[chartTheme?.themeColorCode] || CHART_COLORS[0].colors;
    } else {
      return CHART_COLORS[0].colors;
    }
  }, [chartTheme]);

  const theme = useMemo(() => {
    return {
      seriesCnt: '4',
      backgroundColor: colorBgContainer,
      titleColor: colorText,
      subtitleColor: colorTextSecondary,
      textColorShow: false,
      textColor: colorText,
      markTextColor: colorText,
      borderColor: colorText,
      borderWidth: 0,
      visualMapColor: ['#bf444c', '#d88273', '#333333'],
      legendTextColor: colorText,
      kColor: '#fd1050',
      kColor0: '#0cf49b',
      kBorderColor: '#fd1050',
      kBorderColor0: '#0cf49b',
      kBorderWidth: 1,
      lineWidth: 2,
      symbolSize: 4,
      symbol: 'circle',
      symbolBorderWidth: 1,
      lineSmooth: false,
      graphLineWidth: 1,
      graphLineColor: '#aaa',
      mapLabelColor: '#000',
      mapLabelColorE: 'rgb(100,0,0)',
      mapBorderColor: '#444',
      mapBorderColorE: '#444',
      mapBorderWidth: 0.5,
      mapBorderWidthE: 1,
      mapAreaColor: colorText,
      mapAreaColorE: 'rgba(255,215,0,0.8)',
      axes: [
        {
          type: 'all',
          name: '通用坐标轴',
          axisLineShow: true,
          axisLineColor: colorText,
          axisTickShow: true,
          axisTickColor: colorText,
          axisLabelShow: true,
          axisLabelColor: colorText,
          splitLineShow: true,
          splitLineColor: [colorTextSecondary],
          splitAreaShow: false,
          splitAreaColor: [colorText],
        },
        {
          type: 'category',
          name: '类目坐标轴',
          axisLineShow: true,
          axisLineColor: colorText,
          axisTickShow: true,
          axisTickColor: colorText,
          axisLabelShow: true,
          axisLabelColor: colorText,
          splitLineShow: true,
          splitLineColor: [colorTextSecondary],
          splitAreaShow: false,
          splitAreaColor: ['rgba(250,250,250,0.3)', 'rgba(200,200,200,0.3)'],
        },
        {
          type: 'value',
          name: '数值坐标轴',
          axisLineShow: true,
          axisLineColor: colorText,
          axisTickShow: true,
          axisTickColor: colorText,
          axisLabelShow: true,
          axisLabelColor: colorText,
          splitLineShow: true,
          splitLineColor: [colorTextSecondary],
          splitAreaShow: false,
          splitAreaColor: ['rgba(250,250,250,0.3)', 'rgba(200,200,200,0.3)'],
        },
        {
          type: 'log',
          name: '对数坐标轴',
          axisLineShow: true,
          axisLineColor: colorText,
          axisTickShow: true,
          axisTickColor: colorText,
          axisLabelShow: true,
          axisLabelColor: colorText,
          splitLineShow: true,
          splitLineColor: [colorTextSecondary],
          splitAreaShow: false,
          splitAreaColor: ['rgba(250,250,250,0.3)', 'rgba(200,200,200,0.3)'],
        },
        {
          type: 'time',
          name: '时间坐标轴',
          axisLineShow: true,
          axisLineColor: colorText,
          axisTickShow: true,
          axisTickColor: colorText,
          axisLabelShow: true,
          axisLabelColor: colorText,
          splitLineShow: true,
          splitLineColor: [colorTextSecondary],
          splitAreaShow: false,
          splitAreaColor: ['rgba(250,250,250,0.3)', 'rgba(200,200,200,0.3)'],
        },
      ],
      axisSeperateSetting: false,
      toolboxColor: '#999999',
      toolboxEmphasisColor: '#666666',
      tooltipAxisColor: colorText,
      tooltipAxisWidth: '1',
      timelineLineColor: colorText,
      timelineLineWidth: 1,
      timelineItemColor: '#dd6b66',
      timelineItemColorE: '#a9334c',
      timelineCheckColor: '#e43c59',
      timelineCheckBorderColor: '#c23531',
      timelineItemBorderWidth: 1,
      timelineControlColor: colorText,
      timelineControlBorderColor: colorText,
      timelineControlBorderWidth: 0.5,
      timelineLabelColor: colorText,
      datazoomBackgroundColor: 'rgba(47,69,84,0)',
      datazoomDataColor: 'rgba(255,255,255,0.3)',
      datazoomFillColor: 'rgba(167,183,204,0.4)',
      datazoomHandleColor: '#a7b7cc',
      datazoomHandleWidth: '100',
      datazoomLabelColor: colorText,
      legend: {
        textStyle: {
          color: colorText, // Set the legend text color to dark gray
        },
        pageTextStyle: {
          color: colorText, // Set the legend page information text color to dark gray
        }
      },
      color
    };
  }, [colorPrimary, colorText, color]);

  return { theme };
};
