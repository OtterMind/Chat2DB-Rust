import { IConnectionListItem } from '@/typings/connection';
import { IDatabaseItem as DBItem } from '@/typings/database';
import { ISchemaItem as SchemaItem } from '@/typings/schema';
import { ITable } from '@/typings/tree';
import { IColumn } from '@/typings/column';

export interface IDataSourceItem extends IConnectionListItem {
  dataSourceId: number;
  dataSourceName: string;
  supportDatabase: boolean;
  supportSchema: boolean;
}

export interface IDatabaseItem extends DBItem {
  dataSourceId: number;
  dataSourceName: string;
  databaseName: string;
  supportDatabase: boolean;
  supportSchema: boolean;
}

export interface ISchemaItem extends SchemaItem {
  schemaName: string;
  dataSourceId: number;
  dataSourceName: string;
  databaseName?: string;
}

export interface ITableItem extends ITable {
  dataSourceId: number;
  dataSourceName: string;
  databaseName?: string;
  schemaName?: string;
  tableName: string;
}

export interface IFieldItem extends IColumn {
  fieldName: string;
  tableName: string;
}
