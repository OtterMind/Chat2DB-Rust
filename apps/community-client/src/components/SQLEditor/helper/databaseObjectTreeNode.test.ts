import { TreeNodeType } from '@/constants/tree';
import { TreeNodeData } from '@/typings/tree';
import { findDatabaseObjectTreeNode, getCandidateTreeNodeTypes } from './databaseObjectTreeNode';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

const baseExtraParams = {
  dataSourceId: 7,
  databaseName: 'app',
  schemaName: 'public',
};

const tableNode: TreeNodeData = {
  key: 'dataSource_7-database_app-schema_public-table_users',
  originalTitle: 'users',
  title: null,
  treeNodeType: TreeNodeType.TABLE,
  extraParams: {
    ...baseExtraParams,
    tableName: 'users',
  },
};

const viewNode: TreeNodeData = {
  key: 'dataSource_7-database_app-schema_public-view_v1',
  originalTitle: 'v1',
  title: null,
  treeNodeType: TreeNodeType.VIEW,
  extraParams: {
    ...baseExtraParams,
    tableName: 'v1',
    viewName: 'v1',
  },
};

const databaseLevelFunctionNode: TreeNodeData = {
  key: 'dataSource_7-database_app-function_calc_total-uuid_1',
  originalTitle: 'calc_total',
  title: null,
  treeNodeType: TreeNodeType.FUNCTION,
  extraParams: {
    dataSourceId: 7,
    databaseName: 'app',
    functionName: 'calc_total',
  },
};

const treeData: TreeNodeData[] = [
  {
    key: 'dataSource_7',
    originalTitle: 'local',
    title: null,
    treeNodeType: TreeNodeType.DATA_SOURCE,
    extraParams: {
      dataSourceId: 7,
    },
    children: [
      {
        key: 'dataSource_7-database_app',
        originalTitle: 'app',
        title: null,
        treeNodeType: TreeNodeType.DATABASE,
        extraParams: {
          dataSourceId: 7,
          databaseName: 'app',
        },
        children: [
          {
            key: 'dataSource_7-database_app-schema_public-tables_chat2dbCatalogue',
            originalTitle: 'tables',
            title: null,
            treeNodeType: TreeNodeType.TABLES,
            extraParams: baseExtraParams,
            children: [tableNode],
          },
          {
            key: 'dataSource_7-database_app-schema_public-views_chat2dbCatalogue',
            originalTitle: 'views',
            title: null,
            treeNodeType: TreeNodeType.VIEWS,
            extraParams: baseExtraParams,
            children: [viewNode],
          },
          {
            key: 'dataSource_7-database_app-functions_chat2dbCatalogue',
            originalTitle: 'functions',
            title: null,
            treeNodeType: TreeNodeType.FUNCTIONS,
            extraParams: {
              dataSourceId: 7,
              databaseName: 'app',
            },
            children: [databaseLevelFunctionNode],
          },
        ],
      },
    ],
  },
];

assertEqual(
  getCandidateTreeNodeTypes(TreeNodeType.TABLE),
  [TreeNodeType.TABLE, TreeNodeType.VIEW],
  'table parser identifiers should search tables first and views second',
);

const tableMatch = findDatabaseObjectTreeNode(treeData, {
  treeNodeType: TreeNodeType.TABLE,
  databaseType: 'MYSQL' as any,
  ...baseExtraParams,
  name: 'users',
});

assertEqual(tableMatch?.key, tableNode.key, 'find real table node by constructed key');

const missingTableMatch = findDatabaseObjectTreeNode(treeData, {
  treeNodeType: TreeNodeType.TABLE,
  databaseType: 'MYSQL' as any,
  ...baseExtraParams,
  name: 'v1',
});

assertEqual(missingTableMatch, null, 'do not match a view while searching the table node type');

const viewFallbackMatch = findDatabaseObjectTreeNode(treeData, {
  treeNodeType: TreeNodeType.VIEW,
  databaseType: 'MYSQL' as any,
  ...baseExtraParams,
  name: 'v1',
});

assertEqual(viewFallbackMatch?.key, viewNode.key, 'find real view node when table lookup falls back to view');

const missingRelationMatch = findDatabaseObjectTreeNode(treeData, {
  treeNodeType: TreeNodeType.VIEW,
  databaseType: 'MYSQL' as any,
  ...baseExtraParams,
  name: 'missing_relation',
});

assertEqual(missingRelationMatch, null, 'return null when no real table or view node exists');

const functionMatchWithBlankSchema = findDatabaseObjectTreeNode(treeData, {
  treeNodeType: TreeNodeType.FUNCTION,
  dataSourceId: 7,
  databaseType: 'MYSQL' as any,
  databaseName: 'app',
  schemaName: '',
  name: 'calc_total',
});

assertEqual(
  functionMatchWithBlankSchema?.key,
  databaseLevelFunctionNode.key,
  'find database-level function node when schema is blank in the editor context',
);

const caseInsensitiveTableMatch = findDatabaseObjectTreeNode(treeData, {
  treeNodeType: TreeNodeType.TABLE,
  dataSourceId: 7,
  databaseType: 'MYSQL' as any,
  databaseName: 'APP',
  schemaName: 'PUBLIC',
  name: 'Users',
});

assertEqual(
  caseInsensitiveTableMatch?.key,
  tableNode.key,
  'match table node case-insensitively for non-Oracle databases',
);

const oracleTreeData: TreeNodeData[] = [
  {
    key: 'dataSource_8-database_ORCL-schema_APP-table_USER_ORG',
    originalTitle: 'USER_ORG',
    title: null,
    treeNodeType: TreeNodeType.TABLE,
    extraParams: {
      dataSourceId: 8,
      databaseName: 'ORCL',
      schemaName: 'APP',
      tableName: 'USER_ORG',
    },
  },
];

const oracleUppercaseMatch = findDatabaseObjectTreeNode(oracleTreeData, {
  treeNodeType: TreeNodeType.TABLE,
  dataSourceId: 8,
  databaseType: 'ORACLE' as any,
  databaseName: 'orcl',
  schemaName: 'app',
  name: 'user_org',
});

assertEqual(
  oracleUppercaseMatch?.key,
  oracleTreeData[0].key,
  'match Oracle object names with uppercase semantics',
);

console.log('databaseObjectTreeNode tests passed');
