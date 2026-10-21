import { INormalizedData, ChartSchema } from '@/blocks/BI/Chart/typings';

export interface DataTreatingProps {
  data?: INormalizedData;
  chartSchema: ChartSchema;
  size: {
    width: number;
    height: number;
  };
}

function generateWordCloudData(originalData, size) {
  // calculates the amount of data required (width is proportional to the amount of data, up to 2000)
  let requiredCount = Math.min(size.width / 2, 2000); // no more than 2000 data


  if (requiredCount < 300) {
    requiredCount = 300; // at least 300 data
  }

  // Get the original data volume
  const originalCount = originalData.length;
  let scaledData = [...originalData];

  // If the original data volume is not enough, calculate the amount that needs to be supplemented
  if (originalCount < requiredCount) {
    const scaleFactors = [];
    let factor = 2;

    // dynamically generates multiples, starting from 2 until the required data amount is met
    while (scaledData.length < requiredCount && factor <= 2000) {
      scaleFactors.push(factor);
      factor += 1; // doubles the multiple each time
    }

    // uses dynamic multiples to supplement data
    scaleFactors.forEach((scaleFactor) => {
      originalData.forEach((item) => {
        scaledData.push({ name: item.name, value: item.value / scaleFactor, originalValue: item.value });
      });
    });
  }

  // intercepted to the required number
  scaledData = scaledData.slice(0, requiredCount);

  return scaledData;
}

export const pieDataTreating = (props: DataTreatingProps) => {
  const {
    data,
    chartSchema,
    size = {
      width: 100,
      height: 100,
    },
  } = props;
  const { valueField, textField } = chartSchema;

  let { xField, yField } = chartSchema;
  // dynamically searches for keys in data
  if (data && data.length > 0) {
    xField = Object.keys(data[0]).find((key) => key.toLowerCase() === xField?.toLowerCase()) || xField;
    yField = Object.keys(data[0]).find((key) => key.toLowerCase() === yField?.toLowerCase()) || yField;
  }

  const _xField = xField || textField || 'name';
  const _yField = yField || valueField || 'value';

  let seriesData = data?.map((item) => {
    return {
      name: item[_xField],
      value: item[_yField],
      originalValue: item[_yField],
    };
  });

  if (seriesData?.length && chartSchema.bespreadWordCloud) {
    seriesData = generateWordCloudData(seriesData, size);
  }

  return {
    seriesData,
  };
};
