import { TreeNodeType } from '@/constants/tree';

export const formatObject = (obj: Record<string, any>): Record<string, string | any> => {
  return Object.keys(obj).reduce((acc: Record<string, string | any>, key: string) => {
    // Normalize empty values before generating stable tree keys.
    acc[key] = obj[key] === '' || obj[key] === null ? undefined : obj[key];
    return acc;
  }, {});
};

self.onmessage = function (event) {
  const { type, data } = event.data;
  switch (type) {
    // Process the data passed from the backend
    case 'handlingRes':
      handleHandlingRes(data);
      break;

    default:
      console.error('Unknown message type:', type);
  }
};

function createTreeNodeKey(params) {
  const { dataSourceId, databaseName, schemaName, tableName } = formatObject(params);
  return [`dataSource_${dataSourceId}`, `database_${databaseName}`, `schema_${schemaName}`, `table_${tableName}`].join(
    '-',
  );
}

// Handling handlingRes messages
function handleHandlingRes({ data, extraParams }) {
  const pinnedList: string[] = [];
  const tableList: TreeNodeType[] = [];
  const { dataSourceId, databaseName, schemaName } = extraParams;
  data?.forEach((t: any) => {
    if (!pinnedList.includes(t.name)) {
      const key = createTreeNodeKey!({
        dataSourceId,
        databaseName,
        schemaName,
        tableName: t.name,
      });
      tableList.push({
        key,
        originalTitle: t.name,
        title: null,
        treeNodeType: TreeNodeType.TABLE,
        describe: t.comment,
        extraParams: {
          ...extraParams,
          tableName: t.name,
        },
        decorativeParams: {
          pinned: t.pinned,
          comment: t.comment,
        },
      } as any);
    }
    if (t.pinned) {
      pinnedList.push(t.name);
    }
  });
  self.postMessage({
    type: 'handlingRes',
    data: tableList,
  });
}
