import { DataCollectionElementType } from '@/constants/aiDataCollection';

export interface AiDataCollectionItem {
  id: number;
  title: string;
  elements: AiDataCollectionElement[];
}

export interface AiDataCollectionElement {
  id: number;
  tableName: string;
  dataSourceId: string;
  databaseName: string;
  schemaName: string;
  type: DataCollectionElementType;
}
