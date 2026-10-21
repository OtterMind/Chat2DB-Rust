import { IChartItem } from '@/typings/dashboard';
import { reverseFormattedSqlExecuteData } from '@/utils/dashboard';

export const chartDetailNormalization = (data: IChartItem): IChartItem => {
  if (!data) return data;
  
  const newData: any = {
    ...data,
    chartSchema: {
      ...data.chartSchema,
    },
  };

  // Correct the data, correct the data in the old excel chart to be stored in chartSchema
  if (newData?.chartSchema?.data) {
    newData.metaData = reverseFormattedSqlExecuteData(data.chartSchema!.data);
    delete newData.chartSchema.data;
  }

  // Corrected data, the old name is in chartSchema.title
  if (newData.chartSchema) {
    newData.chartSchema.title = newData.chartSchema.title || newData.name;
  }

  return newData;
};
