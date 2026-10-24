import { IDatabaseBaseInfo } from '@/typings/database';
import importExportServices from '@/service/importExport';
import jcefApi from '@/jcef';

export interface ExportSqlFileProps extends IDatabaseBaseInfo {
  tableNames?: string[];
}

export const generateJavaClass = async (props: ExportSqlFileProps) => {
  const exportPath: string | undefined = await jcefApi?.selectDirectory();

  // does not pop up the file selection box in the web environment, allowing the backend to take the default path
  // if (isDevelopment) {
  //   exportPath = [];
  // }

  if (!exportPath) return;

  const params = {
    ...props,
    exportPath,
  };

  void importExportServices.generateJavaClass(params);
};
