import { IManageResultData } from '@/typings';
import {createChat2dbSpecificSymbolIdentifier} from '@/utils/chat2dbIdentifier';

// Gets each of the given columnsvalueand their number is [{value: 'value', count: 1}]
// is obtained through the resultData of the result set,
export const getColumnValue = (resultData: IManageResultData, field) => {
  const dataList = resultData.dataList || [];
  const index = resultData.headerList.findIndex((item,_index) => _index.toString() === field);
  const values = dataList.map((item) => item[index]?.value ?? null);

  const valueCountMap = values.reduce((acc:any, cur) => {
    const key = createChat2dbSpecificSymbolIdentifier(cur)
    const existingEntry:any = acc.find((entry:any) => entry.key === key);
    if (existingEntry) {
      existingEntry.count += 1;
    } else {
      acc.push({ title: key, key: key, count: 1 });
    }
    return acc;
  }, []);
  return valueCountMap;
};
