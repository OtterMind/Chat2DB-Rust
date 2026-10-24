import { z } from 'zod';

// The Rust product persists UUID strings while the Community client was
// written for numeric ids; both are valid cache keys.
const datasourceIdSchema = z.union([z.number(), z.string()]);

// Validation schema for DataSourceTree
export const DataSourceTreeSchema = z.object({
  datasourceId: datasourceIdSchema,
  hiddenTreeNodeIds: z.array(z.string()),
});

// Schema used to validate input parameters
export const FilterInputSchema = z.object({
  datasourceId: datasourceIdSchema,
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
