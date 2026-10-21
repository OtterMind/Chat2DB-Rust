import { IExcelSheetDataVO } from '@/typings/common';

export const handleExcelData = (excelData: IExcelSheetDataVO[]): any | null => {
  if (!excelData) {
    return null;
  }
  return excelData.map((sheet) => {
    const columns: any = [
      {
        title: '',
        dataIndex: '0',
        fixed: 'left',
        width: 50,
        align: 'center',
        className: 'backgroundColorFillQ',
      },
    ];
    sheet.dataList?.[0]?.map((header: any, index) => {
      columns.push({
        dataIndex: index + 1,
        title: index + 1,
        width: 100,
        className: 'backgroundColorFillQ',
      });
    });
    const dataSource = sheet.dataList?.map((data: any, index) => {
      const dataTemplate: any = {
        '0': index + 1,
      };
      data.map((item: any, i: number) => {
        dataTemplate[String(i + 1)] = item;
      });
      return dataTemplate;
    });

    const {
      sheetNo,
      sheetName,
      tableName,
      del,
      headerEndColNum,
      headerEndRowNum,
      headerStartColNum,
      headerStartRowNum,
      tableType,
    } = sheet;

    let headerNumScope: number[] = [];

    if (sheet.tableType === 'horizontal') {
      headerNumScope = [headerStartRowNum + 1, headerEndRowNum + 1];
    } else {
      headerNumScope = [headerStartColNum + 1, headerEndColNum + 1];
    }

    return {
      sheetNo,
      columns,
      dataSource,
      sheetName,
      tableName,
      headerNumScope,
      tableType: tableType || 'horizontal',
      del,
    };
  });
};
