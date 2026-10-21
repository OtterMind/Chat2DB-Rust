import { SqlStatement, SqlTypeEnum, StatementValidTypeEnum } from '@/typings/sqlParser';
import { findTableIdentifierAtPosition, getTableIdentifierUnderlineRanges } from './tableIdentifier';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function createStatement(params: {
  sql: string;
  startRow?: number;
  endRow?: number;
  sqlType?: SqlTypeEnum;
  tableName: string;
  identifierType?: string;
  identifierStartColNum: number;
  identifierEndColNum: number;
  identifierDatabase?: string;
  identifierSchema?: string;
}): SqlStatement {
  const startRow = params.startRow || 1;
  const endRow = params.endRow || startRow;
  return {
    sql: params.sql,
    sqlStartRowNum: startRow,
    sqlStartColNum: 1,
    sqlEndRowNum: endRow,
    sqlEndColNum: params.sql.length + 1,
    type: params.sqlType || SqlTypeEnum.SELECT,
    statementType: StatementValidTypeEnum.VALID,
    comment: '',
    identifiers: [
      {
        name: params.tableName,
        alias: '',
        type: params.identifierType || 'TABLE',
        identifierDatabase: params.identifierDatabase || '',
        identifierSchema: params.identifierSchema || '',
        identifierTable: params.tableName,
        identifierStartRowNum: startRow,
        identifierStartColNum: params.identifierStartColNum,
        identifierEndRowNum: startRow,
        identifierEndColNum: params.identifierEndColNum,
        aliasStartRowNum: 0,
        aliasStartColNum: 0,
        aliasEndRowNum: 0,
        aliasEndColNum: 0,
      },
    ],
    tableColumns: [],
  };
}

function createIdentifier(params: {
  name: string;
  type?: string;
  identifierTable?: string;
  alias?: string;
  identifierStartColNum: number;
  identifierEndColNum: number;
  aliasStartColNum?: number;
  aliasEndColNum?: number;
  startRow?: number;
}) {
  const startRow = params.startRow || 1;
  return {
    name: params.name,
    alias: params.alias || '',
    type: params.type || 'TABLE',
    identifierDatabase: '',
    identifierSchema: '',
    identifierTable: params.identifierTable || params.name,
    identifierStartRowNum: startRow,
    identifierStartColNum: params.identifierStartColNum,
    identifierEndRowNum: startRow,
    identifierEndColNum: params.identifierEndColNum,
    aliasStartRowNum: params.aliasStartColNum ? startRow : 0,
    aliasStartColNum: params.aliasStartColNum || 0,
    aliasEndRowNum: params.aliasEndColNum ? startRow : 0,
    aliasEndColNum: params.aliasEndColNum || 0,
  };
}

const baseDbInfo = {
  dataSourceId: 7,
  dataSourceName: 'local',
  databaseType: 'MYSQL' as any,
  databaseName: 'app',
  schemaName: 'public',
};

function pickTableIdentifier(
  sqlStatementList: SqlStatement[],
  lineNumber: number,
  column: number,
  dbInfo = baseDbInfo,
) {
  const tableIdentifier = findTableIdentifierAtPosition({ lineNumber, column }, sqlStatementList, dbInfo);
  return (
    tableIdentifier && {
      dataSourceId: tableIdentifier.dataSourceId,
      dataSourceName: tableIdentifier.dataSourceName,
      databaseType: tableIdentifier.databaseType,
      databaseName: tableIdentifier.databaseName,
      schemaName: tableIdentifier.schemaName,
      objectType: tableIdentifier.objectType,
      tableName: tableIdentifier.tableName,
    }
  );
}

const simpleStatement = createStatement({
  sql: 'select * from users;',
  tableName: 'users',
  identifierStartColNum: 15,
  identifierEndColNum: 20,
});

assertEqual(
  pickTableIdentifier([simpleStatement], 1, 16),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'public',
    objectType: 'TABLE',
    tableName: 'users',
  },
  'resolve table identifier under cursor',
);

assertEqual(pickTableIdentifier([simpleStatement], 1, 3), null, 'ignore non-table token position');

const schemaStatement = createStatement({
  sql: 'select * from public.users u;',
  tableName: 'users',
  identifierStartColNum: 22,
  identifierEndColNum: 27,
  identifierSchema: 'public',
});

assertEqual(
  pickTableIdentifier([schemaStatement], 1, 17),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'public',
    objectType: 'TABLE',
    tableName: 'users',
  },
  'resolve schema.table when cursor is on schema',
);

assertEqual(
  pickTableIdentifier([schemaStatement], 1, 21),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'public',
    objectType: 'TABLE',
    tableName: 'users',
  },
  'resolve schema.table when cursor is on dot',
);

assertEqual(
  getTableIdentifierUnderlineRanges([schemaStatement]),
  [
    {
      startLineNumber: 1,
      startColumn: 15,
      endLineNumber: 1,
      endColumn: 27,
    },
  ],
  'underline full qualified table identifier',
);

const quotedStatement = createStatement({
  sql: 'select * from `sales-db`.`order-items`;',
  tableName: 'order-items',
  identifierStartColNum: 27,
  identifierEndColNum: 40,
  identifierDatabase: 'sales-db',
});

assertEqual(
  pickTableIdentifier([quotedStatement], 1, 16),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'sales-db',
    schemaName: 'public',
    objectType: 'TABLE',
    tableName: 'order-items',
  },
  'resolve backtick qualified identifier',
);

const doubleQuotedStatement = createStatement({
  sql: 'update "crm"."Account" set name = ?;',
  tableName: 'Account',
  identifierStartColNum: 15,
  identifierEndColNum: 24,
  identifierSchema: 'crm',
});

assertEqual(
  pickTableIdentifier([doubleQuotedStatement], 1, 10),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'crm',
    objectType: 'TABLE',
    tableName: 'Account',
  },
  'resolve double quoted qualified identifier',
);

const statements: SqlStatement[] = [
  simpleStatement,
  createStatement({
    sql: 'delete from archive.orders where id = 1;',
    startRow: 2,
    endRow: 2,
    tableName: 'orders',
    identifierStartColNum: 21,
    identifierEndColNum: 27,
    identifierSchema: 'archive',
  }),
];

assertEqual(
  pickTableIdentifier(statements, 2, 13),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'archive',
    objectType: 'TABLE',
    tableName: 'orders',
  },
  'resolve identifier in current statement only',
);

const tableAliasStatement: SqlStatement = {
  sql: 'select * from access_control_apply_record a where a.id=2 order by a.id;',
  sqlStartRowNum: 1,
  sqlStartColNum: 1,
  sqlEndRowNum: 1,
  sqlEndColNum: 72,
  type: SqlTypeEnum.SELECT,
  statementType: StatementValidTypeEnum.VALID,
  comment: '',
  identifiers: [
    createIdentifier({
      name: 'access_control_apply_record',
      alias: 'a',
      identifierStartColNum: 15,
      identifierEndColNum: 42,
      aliasStartColNum: 43,
      aliasEndColNum: 44,
    }),
    createIdentifier({
      name: 'a',
      identifierStartColNum: 51,
      identifierEndColNum: 52,
    }),
    createIdentifier({
      name: 'a',
      identifierStartColNum: 67,
      identifierEndColNum: 68,
    }),
  ],
  tableColumns: [],
};

assertEqual(
  pickTableIdentifier([tableAliasStatement], 1, 16),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'public',
    objectType: 'TABLE',
    tableName: 'access_control_apply_record',
  },
  'resolve real table name when statement has table alias references',
);

assertEqual(pickTableIdentifier([tableAliasStatement], 1, 43), null, 'ignore table alias definition token');

assertEqual(
  pickTableIdentifier([tableAliasStatement], 1, 51),
  null,
  'ignore table alias qualifier in where column reference',
);

assertEqual(
  pickTableIdentifier([tableAliasStatement], 1, 67),
  null,
  'ignore table alias qualifier in order by column reference',
);

assertEqual(
  getTableIdentifierUnderlineRanges([tableAliasStatement]),
  [
    {
      startLineNumber: 1,
      startColumn: 15,
      endLineNumber: 1,
      endColumn: 42,
    },
  ],
  'underline only real table name and not alias qualifiers',
);

const viewStatement = createStatement({
  sql: 'select * from v1;',
  tableName: 'v1',
  identifierType: 'VIEW',
  identifierStartColNum: 15,
  identifierEndColNum: 17,
});

assertEqual(
  pickTableIdentifier([viewStatement], 1, 16),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'public',
    objectType: 'VIEW',
    tableName: 'v1',
  },
  'resolve view identifier under cursor',
);

const functionStatement = createStatement({
  sql: 'select calculate_total();',
  tableName: 'calculate_total',
  identifierType: 'FUNCTION',
  identifierStartColNum: 8,
  identifierEndColNum: 23,
});

assertEqual(
  pickTableIdentifier([functionStatement], 1, 12),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'public',
    objectType: 'FUNCTION',
    tableName: 'calculate_total',
  },
  'resolve function identifier under cursor',
);

const procedureStatement = createStatement({
  sql: 'call refresh_stats();',
  tableName: 'refresh_stats',
  identifierType: 'PROCEDURE',
  identifierStartColNum: 6,
  identifierEndColNum: 19,
});

assertEqual(
  pickTableIdentifier([procedureStatement], 1, 10),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'public',
    objectType: 'PROCEDURE',
    tableName: 'refresh_stats',
  },
  'resolve procedure identifier under cursor',
);

const createTableStatement = createStatement({
  sql: 'create table Users (id bigint);',
  sqlType: SqlTypeEnum.CREATE_TABLE,
  tableName: 'Users',
  identifierStartColNum: 14,
  identifierEndColNum: 19,
});

assertEqual(
  pickTableIdentifier([createTableStatement], 1, 15),
  null,
  'ignore create table target identifier because it cannot navigate to an existing node',
);

const alterTableStatement = createStatement({
  sql: 'alter table public.Users add column age int;',
  sqlType: SqlTypeEnum.ALTER,
  tableName: 'Users',
  identifierStartColNum: 20,
  identifierEndColNum: 25,
  identifierSchema: 'public',
});

assertEqual(
  pickTableIdentifier([alterTableStatement], 1, 16),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'public',
    objectType: 'TABLE',
    tableName: 'Users',
  },
  'resolve alter table qualified target identifier under cursor',
);

const dropTableStatement = createStatement({
  sql: 'drop table if exists archive.Users;',
  sqlType: SqlTypeEnum.DROP_TABLE,
  tableName: 'Users',
  identifierStartColNum: 30,
  identifierEndColNum: 35,
  identifierSchema: 'archive',
});

assertEqual(
  pickTableIdentifier([dropTableStatement], 1, 25),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'MYSQL',
    databaseName: 'app',
    schemaName: 'archive',
    objectType: 'TABLE',
    tableName: 'Users',
  },
  'resolve drop table qualified target identifier under cursor',
);

const oracleDbInfo = {
  ...baseDbInfo,
  databaseType: 'ORACLE' as any,
  databaseName: 'ORCL',
  schemaName: 'APP',
};

const oracleLowercaseStatement = createStatement({
  sql: 'select * from user_org;',
  tableName: 'user_org',
  identifierStartColNum: 15,
  identifierEndColNum: 23,
});

assertEqual(
  pickTableIdentifier([oracleLowercaseStatement], 1, 16, oracleDbInfo),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'ORACLE',
    databaseName: 'ORCL',
    schemaName: 'APP',
    objectType: 'TABLE',
    tableName: 'USER_ORG',
  },
  'resolve Oracle unquoted lowercase table identifier as uppercase',
);

const oracleSchemaStatement = createStatement({
  sql: 'select * from app.user_org;',
  tableName: 'user_org',
  identifierStartColNum: 19,
  identifierEndColNum: 27,
  identifierSchema: 'app',
});

assertEqual(
  pickTableIdentifier([oracleSchemaStatement], 1, 16, oracleDbInfo),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'ORACLE',
    databaseName: 'ORCL',
    schemaName: 'APP',
    objectType: 'TABLE',
    tableName: 'USER_ORG',
  },
  'resolve Oracle unquoted schema.table identifiers as uppercase',
);

const oracleQuotedStatement = createStatement({
  sql: 'select * from "user_org";',
  tableName: 'user_org',
  identifierStartColNum: 16,
  identifierEndColNum: 24,
});

assertEqual(
  pickTableIdentifier([oracleQuotedStatement], 1, 17, oracleDbInfo),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'ORACLE',
    databaseName: 'ORCL',
    schemaName: 'APP',
    objectType: 'TABLE',
    tableName: 'user_org',
  },
  'preserve Oracle double quoted table identifier case',
);

const oracleQuotedSchemaStatement = createStatement({
  sql: 'select * from "app"."user_org";',
  tableName: 'user_org',
  identifierStartColNum: 22,
  identifierEndColNum: 30,
  identifierSchema: 'app',
});

assertEqual(
  pickTableIdentifier([oracleQuotedSchemaStatement], 1, 17, oracleDbInfo),
  {
    dataSourceId: 7,
    dataSourceName: 'local',
    databaseType: 'ORACLE',
    databaseName: 'ORCL',
    schemaName: 'app',
    objectType: 'TABLE',
    tableName: 'user_org',
  },
  'preserve Oracle double quoted schema and table identifier case',
);

console.log('tableIdentifier tests passed');
