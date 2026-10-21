import { CHART_COLORS, AUTO_REFRESH, OrderByType, OrderByRule } from '@/blocks/BI/Chart/constants';
import { ChartSchema } from '../../typings';

const initialValues = {
  themeColorCode: CHART_COLORS[0].value,
  autoRefresh: {
    refreshRule: AUTO_REFRESH.NEVER,
    minutesRefresh: 15,
    everydayRefresh: '08:30',
  },
  chartOptionCheckbox: ['showAxisLine', 'showSplitLine', 'showSymbol'] as any,
  orderByType: OrderByType.DEFAULT,
  orderByRule: OrderByRule.ASC,
  isSmooth: false,
  bespreadWordCloud: true,
};

export const formToSchema = (formData) => {
  return formData;
};

export const schemaToForm = (chartSchema) => {
  return {
    ...initialValues,
    ...chartSchema,
  };
};

// The schema returned in the later section may lack some default values. Please complete them here.
export const completeSchema = (chartSchema?: ChartSchema): ChartSchema | undefined => { 
  return {
    ...initialValues,
    ...chartSchema,
  };
} 
