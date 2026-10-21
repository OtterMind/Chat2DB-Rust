import { IDatabaseBaseInfo } from '@/typings/database';
import { ImportExportType, ImportExportTaskType, ImportExportTaskStatus } from '@/constants/importExport';

export interface ImportExportDataBoundInfo extends IDatabaseBaseInfo {
  tableName: string;
  type: ImportExportType;
}

export interface ImportExportTaskDetails {
  id: number;
  taskName: string;
  taskType: ImportExportTaskType;
  taskStatus: ImportExportTaskStatus;
  taskProgress: string;
  progress: number;
  downloadUrl: string;
  gmtCreate: number;
  infoLog?: string;
  errorLog?: string;
}
