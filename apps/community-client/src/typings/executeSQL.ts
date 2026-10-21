export interface IExecuteSqlParams {
  sql?: string;
  single?: boolean;
  dataSourceId?: number;
  databaseName?: string;
  schemaName?: string;
  tableName?: string;
  pageNo?: number;
  pageSize?: number;

  // Approval ID
  applyId?: number;
}
