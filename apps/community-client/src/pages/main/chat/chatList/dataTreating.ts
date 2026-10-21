import { AnswerPartsStatus, AnswerPartsType } from '@/constants/chat';
import { AnswerParts } from '@/typings/chat';
import {v4 as uuid} from 'uuid';

export const answersContentToParts = ({ parts, content }:any): AnswerParts[] => {
  
  if (parts) {
    return parts;
  }

  let oldParts = [];
  let oldMarkdownText = '';
  try {
    oldParts = JSON.parse(content || '[]');
  } catch (e) {
    oldMarkdownText = content;
    console.error(e);
  }

  if (oldMarkdownText) { 
    return [{
      id: uuid() as any,
      partType: AnswerPartsType.MARKDOWN,
      text: oldMarkdownText,
      status: AnswerPartsStatus.FINISH
    }];
  }

  return oldParts?.map((item: any) => {
    return {
      id: uuid() as any,
      partType: AnswerPartsType.DASHBOARD,
      databaseInfo: {
        dataSourceId: item.dataSourceId,
        dataSourceName: item.dataSourceName,
        databaseType: item.databaseType,
        databaseName: item.databaseName,
        schemaName: item.schemaName,
        connectable: item.connectable,
        sql: item.ddl,
      },
      chartSchema: {
        ...JSON.parse(item.schema || '{}'),
        title: item.name,
      },
      // metaData?: INormalizedData;
      status: AnswerPartsStatus.FINISH
    };
  }) || [];
};


