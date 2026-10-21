// handles legend configuration according to chartSchema
import _ from 'lodash';
import { ChartSchema } from '@/blocks/BI/Chart/typings';

const handleOptions = (chartSchema: ChartSchema, option: any) => {
  const newOption = {
    ...option,
  };
  const { chartOptionCheckbox = [] } = chartSchema;

  // handles legend configuration according to chartSchema
  if (chartOptionCheckbox.includes('showLegend')) {
    newOption.legend = {
      type: 'scroll',
      left: 'center',
    };
    _.set(newOption, 'grid.top', '10%');
  }

  // Processes whether to display label configuration according to chartSchema
  if (!chartOptionCheckbox.includes('showLabel')) {
    newOption.series.forEach((item: any) => {
      _.set(item, 'label.show', false);
    });
  }

  // Processes whether to display grid line configuration according to chartSchema
  if (!chartOptionCheckbox.includes('showSplitLine')) {
    if (Array.isArray(newOption.xAxis)) {
      newOption.xAxis.forEach((item) => {
        _.set(item, 'splitLine.show', false);
      });
    } else {
      _.set(newOption, 'xAxis.splitLine.show', false);
    }
    if (Array.isArray(newOption.yAxis)) {
      _.set(newOption, 'yAxis[0].splitLine.show', false);
    } else {
      _.set(newOption, 'yAxis.splitLine.show', false);
    }
  }

  // Processes whether to display coordinate axis configuration according to chartSchema
  if (!chartOptionCheckbox.includes('showAxisLine')) {
    if (Array.isArray(newOption.xAxis)) {
      newOption.xAxis.forEach((item) => {
        _.set(item, 'axisLine.show', false);
        _.set(item, 'axisLabel.show', false);
        _.set(item, 'axisTick.show', false);
      });
    } else {
      _.set(newOption, 'xAxis.axisLine.show', false);
      _.set(newOption, 'xAxis.axisLabel.show', false);
      _.set(newOption, 'xAxis.axisTick.show', false);
    }

    if (Array.isArray(newOption.yAxis)) {
      _.set(newOption, 'yAxis[0].axisLine.show', false);
      _.set(newOption, 'yAxis[0].axisLabel.show', false);
      _.set(newOption, 'yAxis[0].axisTick.show', false);
    } else {
      _.set(newOption, 'yAxis.axisLine.show', false);
      _.set(newOption, 'yAxis.axisLabel.show', false);
      _.set(newOption, 'yAxis.axisTick.show', false);
    }
  }

  // Processes whether to display coordinate axis configuration according to chartSchema
  if (!chartOptionCheckbox.includes('showSymbol')) {
    newOption.series.forEach((item) => {
      _.set(item, 'symbolSize', item?.type === 'scatter' ? '14' : '0');
    });
  }

  return newOption;
};

export default handleOptions;
