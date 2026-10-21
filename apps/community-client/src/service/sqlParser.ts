import {
  SqlStatement,
  MarkMessage,
  SimpleIdentifier,
  ISimpleDatabaseVO,
  ISimpleSchemaVO,
  ISimpleTableVO,
  ISimpleViewVO,
  ISimpleFunctionVO,
  ISimpleProcedureVO,
  IHoverInfo,
  ISqlCompletionActiveSnippetSlot,
  SqlCompletionKeywordCase,
  ISqlCompletionResult,
} from '@/typings/sqlParser';
import createRequest from './base';

const prefix = '/api/sql_parser';

/**
 * Parse SQL statements
 */
const querySQLParser = createRequest<
  {
    consoleId: number;
    dataSourceId: number;
    databaseName?: string;
    schemaName?: string;
    sql: string;
  },
  {
    sqlStatementList: SqlStatement[];
    markMessageList: MarkMessage[];
  }
>(`${prefix}/context/parser`, {
  method: 'post',
  errorLevel: false,
});

const queryQuickSQLParser = createRequest<
  { consoleId: number; dataSourceId: number; databaseName?: string; schemaName?: string; sql: string },
  { sqlStatementList: SqlStatement[]; markMessageList: MarkMessage[] }
>(`${prefix}/context/quick_parser`, {
  method: 'post',
  errorLevel: false,
});

/**
 * Get database and schema
 */
const queryDatabaseAndSchema = createRequest<
  { consoleId: number; dataSourceId: number; databaseName?: string; schemaName?: string },
  {
    databases: ISimpleDatabaseVO[];
    schemas: ISimpleSchemaVO[];
    tables: ISimpleTableVO[];
    views: ISimpleViewVO[];
    functions: ISimpleFunctionVO[];
    procedures: ISimpleProcedureVO[];
  }
>(`${prefix}/get_keywords`, {
  errorLevel: false,
});

/**
 * Get tips
 */
const queryTips = createRequest<
  {
    consoleId: number;
    dataSourceId: number;
    databaseName?: string;
    schemaName?: string;

    /** Whether a fully qualified name is required */
    needFullName?: boolean;
    /** SQL keyword presentation case for backend-owned completion candidates */
    keywordCase?: SqlCompletionKeywordCase;
    /** Current Monaco snippet placeholder slot */
    activeSnippetSlot?: ISqlCompletionActiveSnippetSlot;
  } & (
    | {
        /** Complete SQL in the current editor */
        sql: string;
        /** The offset of the cursor in the complete SQL */
        cursor: number;
      }
    | {
        /** The previous part of the cursor in the current sql sql */
        beforeSql: string;
        /** The part of sql behind the cursor in the current sql */
        afterSql: string;
      }
  ),
  ISqlCompletionResult
>(`${prefix}/context/tip`, {
  method: 'post',
  errorLevel: false,
});

const queryHover = createRequest<
  {
    consoleId: number;
    /** All sql in the current editor */
    sql: string;
    /** The complete status of the parsed SQL of the current cursor hover */
    currentStatement: SqlStatement;
    hoverIdentifier: SimpleIdentifier;
    dataSourceId: number;
    databaseName?: string;
    schemaName?: string;
  },
  Array<IHoverInfo>
>(`${prefix}/context/hover`, {
  method: 'post',
  errorLevel: false,
});

export default { querySQLParser, queryQuickSQLParser, queryDatabaseAndSchema, queryTips, queryHover };
