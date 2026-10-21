// enum type
export enum ImportExportType {
  IMPORT = 'import',
  EXPORT = 'export',
}

export enum ImportExportFileType {
  CSV = 'CSV',
  XLS = 'XLS',
  XLSX = 'XLSX',
  SQL = 'SQL',
}

export enum ImportExportTaskType {
  DOWNLOAD_DATA = 'DOWNLOAD_DATA', // Download data
  DOWNLOAD_TABLE_STRUCTURE = 'DOWNLOAD_TABLE_STRUCTURE', // Download table structure
  UPLOAD_TABLE_DATA = 'UPLOAD_TABLE_DATA', // Upload table data
  UPLOAD_TABLE_STRUCTURE = 'UPLOAD_TABLE_STRUCTURE', // Upload table structure
}

export enum ImportExportTaskStatus {
  INIT = 'INIT', // initialization
  PROCESSING = 'PROCESSING', // Processing
  RUNNING = 'RUNNING', // Processing
  FINISHED = 'FINISHED', // Complete
  ERROR = 'ERROR', // Error
  STOP = 'STOP', // Error
}
