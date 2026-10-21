import { memo, useState, useMemo } from 'react';
import EChartsContainer from '@/blocks/BI/Chart/components/EChartsContainer';
import { useStyles } from './style';
import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';
import { pieDataTreating } from './dataTreating';
import handleOptions from '../../chartOptionUtils/handleOption';
import { wordCloudChartShapeMap } from '../../constants/wordCloudChartShape';
import { WordCloudChartShape } from '../../constants';
import { CHART_COLORS_MAP } from '@/blocks/BI/Chart/constants';
import _ from 'lodash';
import i18n from '@/i18n';

export interface BarChartProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
}

const WordCloudChart = (props: BarChartProps) => {
  const { data, chartSchema } = props;

  const [size, setSize] = useState({ width: 0, height: 0 });

  const { theme } = useStyles();
  const { fontFamily, colorBgContainer, colorBorderSecondary, colorText, colorTextSecondary, colorPrimary } = theme;

  const colors = useMemo(() => {
    return chartSchema?.themeColorCode
      ? CHART_COLORS_MAP[chartSchema?.themeColorCode] || CHART_COLORS_MAP['v1-colorful-1']
      : CHART_COLORS_MAP['v1-colorful-1'];
  }, [chartSchema?.themeColorCode]);

  const { seriesData } = pieDataTreating({ data, chartSchema, size });

  const { image: symbolUrl } = wordCloudChartShapeMap[WordCloudChartShape.CLOUD];

  const maskImage = useMemo(() => {
    const _maskImage = new Image();
    _maskImage.src = symbolUrl;
    return _maskImage;
  }, []);

  const computeMinSize = (_size) => {
    const width = Math.min(_size.width, _size.height);
    if (width < 600) {
      return 8;
    } else if (width < 800) {
      return 10;
    } else if (width < 1000) {
      return 12;
    } else {
      return 14;
    }
  };

  const computeGridSize = (_size) => {
    const width = Math.min(_size.width, _size.height);
    if (width < 600) {
      return 12;
    } else if (width < 800) {
      return 16;
    } else if (width < 1000) {
      return 18;
    } else {
      return 20;
    }
  };

  const option = useMemo(() => {
    if (!size.height) {
      return undefined;
    }

    const minSize = computeMinSize(size);
    const maxSize = Math.min(size.height / 4, size.width / 4);
    const sizeRange = [minSize, maxSize];

    let _option = {
      tooltip: {
        show: true,
        trigger: 'item',
        backgroundColor: colorBgContainer, // white background
        borderColor: colorBorderSecondary, // border color
        borderWidth: 1, // border width
        padding: [10, 15], // padding
        textStyle: {
          color: colorText, // default text color
          fontSize: 14, // font size
        },
        formatter: function (params) {
          // Define the HTML style of the content
          return `
            <div style="font-family: Arial, sans-serif; font-size: 14px; color: ${colorText};">
              <div style="margin-bottom: 4px; font-weight: bold;">${params.name}</div>
              <div style="display: flex; align-items: center;">
                <span style="
                  width: 4px;
                  height: 40px;
                  background-color: ${colorPrimary};
                  display: inline-block;
                  margin-right: 8px;
                  border-radius: 2px;
                "></span>
                <div>
                  <div style="color: ${colorTextSecondary}; font-size: 12px;">${i18n('workspace.text.frequency')}</div>
                  <div style="font-size: 14px;">${params.data.originalValue}</div>
                </div>
              </div>
            </div>
          `;
        },
      },
      grid: {
        left: 0,
        bottom: 0,
        top: 0,
        right: 0,
      },
      series: [
        {
          type: 'wordCloud',
          shape: 'circle',
          keepAspect: true,
          maskImage: maskImage,
          left: 'center',
          top: 'center',
          width: '90%',
          height: '90%',
          right: null,
          bottom: null,
          sizeRange,
          rotationRange: [0, 0],
          gridSize: computeGridSize(size),
          drawOutOfBound: false,
          shrinkToFit: false,
          layoutAnimation: true,
          textStyle: {
            fontFamily,
            color: function (item) {
              const color = colors[item.dataIndex % colors.length]; // Cycle through the color array.
              return color;
            },
          },
          // emphasis: {
          //   focus: 'self',
          //   textStyle: {
          //     textShadowBlur: 10,
          //     textShadowColor: colorPrimary,
          //   },
          // },
          emphasis: {
            focus: 'self',
            textStyle: {
              textShadowBlur: 10,
              color: colorPrimary,
            },
          },
          data: seriesData,
        },
      ],
    };

    if (chartSchema) {
      _option = handleOptions(chartSchema, _option);
      // Remove xAxis and yAxis.
      _.unset(_option, 'xAxis');
      _.unset(_option, 'yAxis');
    }

    return _option;
  }, [seriesData, size, colors]);

  return (
    <EChartsContainer
      resizeRefresh={false}
      optionsChangeRefresh={true}
      resizeCallback={(width, height) => {
        setSize({ width, height });
      }}
      option={option}
      chartSchema={chartSchema}
    />
  );
};

export default memo(WordCloudChart);
