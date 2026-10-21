import { z } from 'zod';

// Validation schema for DataSourceTree
export const DataSourceTreeSchema = z.object({
  datasourceId: z.number(),
  hiddenTreeNodeIds: z.array(z.string()),
});

// Schema used to validate input parameters
export const FilterInputSchema = z.object({
  datasourceId: z.number(),
  hiddenTreeNodeIds: z.array(z.string()).optional(),
});

export type DataSourceTreeSchemaInput = z.infer<typeof FilterInputSchema>;

export const validateData = <T>(data: T, schema: z.ZodType<T>): void => {
  try {
    schema.parse(data);
  } catch (error) {
    if (error instanceof z.ZodError) {
      throw new Error(`Data validation failed: ${error.message}`);
    }
    throw error;
  }
};
