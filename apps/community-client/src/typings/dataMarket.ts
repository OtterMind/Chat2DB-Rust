export enum CollectionSourceDataType {
  DATA_SOURCE = 'DATA_SOURCE',
  EXCEL = 'EXCEL',
}
export interface ICreateCollectionRequest {
  /**
   * Collection title
   */
  title: string;
  /**
   *Collect data sources,
   */
  collectionSource: CollectionSourceDataType;
  /**
   * Collection elements
   */
  elements: ICollectionElement[];
}
/**
 * Create data elements
 */
export interface ICollectionElement {
  /**
   *Data source ID
   */
  dataSourceId: number;
  /**
   * Database name
   */
  databaseName: string;
  /**
   * schema name
   */
  schemaName: string;
  /**
   *Table name
   */
  tableName: string;
}

export interface IDataSourceCollection {
  /**
   * Primary key
   */
  id: number;
  /**
   * Collection title
   */
  title: string;
  /**
   * Collection data source, DATA_SOURCE/EXCEL
   */
  collectionSource: string;
  /**
   * Collection elements
   */
  elements: IDataSourceCollectionElement[];
  /**
   * Create user ID
   */
  createUserId: number;
  /**
   * Modify user ID
   */
  modifyUserId: number;
  /**
   *Organization ID
   */
  organizationId: number;
}

export interface IDataSourceCollectionElement {
  /**
   * Primary key
   */
  id: number;
  /**
   * Data source collection ID
   */
  dataSourceCollectionId: number;
  /**
   *Data source ID
   */
  dataSourceId: number;
  /**
   * Database name
   */
  databaseName: string;
  /**
   * schema name
   */
  schemaName: string;
  /**
   *Table name
   */
  tableName: string;
  /**
   * Creation time
   */
  createTime: number;
  /**
   * Create user ID
   */
  createUserId: number;
  /**
   * Update time
   */
  modifyTime: number;
  /**
   * Modify user ID
   */
  modifyUserId: number;
  /**
   *Organization ID
   */
  organizationId: number;
}
