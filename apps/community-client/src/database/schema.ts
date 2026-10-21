// ************************************** //
// ******* Version 1 - 2025-02-13 ******* //
// ************************************** //
// - Initial database schema with `dataSourceTree` table

import { z } from 'zod';

export const dbSchemaV1 = {
  dataSourceTree: '&datasourceId',
};

export const treeNodeShowTreeNodeIdsSchema = z.record(z.string(), z.array(z.string()));
