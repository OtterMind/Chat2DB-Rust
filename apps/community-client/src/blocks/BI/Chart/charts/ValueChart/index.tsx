import React, { memo, useRef, useMemo, useCallback } from 'react';
import { useStyles } from './style';
import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';
import { pieDataTreating } from './dataTreating';
import { useSize, useUpdateEffect } from 'ahooks';
import { useChartTheme } from '../../hooks/useTheme';
import _ from 'lodash';

export interface BarChartProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
}

function calculateFontSize(width, height, seriesData) {
  // creates a hidden div
  const div = document.createElement('div');
  div.style.visibility = 'hidden';
  div.style.position = 'absolute';
  div.style.whiteSpace = 'nowrap';
  document.body.appendChild(div);

  // sets a proportion coefficient to adjust the relationship between font size and div size
  const scaleFactor = 0.3;

  // Choose a smaller size to prevent the font from being too large
  const minDimension = Math.min(width, height);

  // Calculate font size
  let fontSize = minDimension * scaleFactor;

  // If the font size exceeds 150px, set it to 150px
  if (fontSize > 150) {
    fontSize = 150;
  }

  // sets the font size of div
  div.style.fontSize = `${fontSize}px`;

  // Add the content to the div.
  div.textContent = seriesData;

  // Measure the actual content width.
  let contentWidth = div.offsetWidth;

  // Reduce the font size when the content exceeds 90% of the available width.
  while (contentWidth > width * 0.8) {
    fontSize -= 1;
    div.style.fontSize = `${fontSize}px`;
    contentWidth = div.offsetWidth;
  }

  // Remove the div.
  document.body.removeChild(div);

  return fontSize;
}

const ValueChart = (props: BarChartProps) => {
  const { data, chartSchema } = props;
  const wrapperRef = useRef<HTMLDivElement>(null);
  const valueRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = React.useState(0);
  const { seriesData } = pieDataTreating({ data, chartSchema });
  // Observe container-size changes.
  const wrapperSize = useSize(wrapperRef);
  const { theme } = useChartTheme({ themeColorCode: chartSchema?.themeColorCode });
  const color = useMemo(() => theme?.color?.[0], [theme]);
  const { styles } = useStyles({ size, color });

  const throttleResize = useCallback(
    _.throttle((width, height) => {
      // Skip resizing when the container has no dimensions.
      if (width && height) {
        setSize(calculateFontSize(width, height, seriesData));
      }
    }, 400),
    [],
  );

  useUpdateEffect(() => {
    const { width, height } = wrapperSize as any;
    // Skip resizing when the container has no dimensions.
    if (width && height) {
      throttleResize(width, height);
    }
  }, [wrapperSize]);

  return (
    <div ref={wrapperRef} className={styles.valueContainer}>
      <div ref={valueRef}>{!!size && seriesData}</div>
    </div>
  );
};

export default memo(ValueChart);
