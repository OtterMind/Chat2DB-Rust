import { DatabaseTypeCode } from '@/constants';
import MYSQL from './mysql';
import ORACLE from './oracle';
import SQLServer from './sqlServer';
import PG from './pg';

export default {
  [DatabaseTypeCode.MYSQL]: MYSQL,
  [DatabaseTypeCode.CLICKHOUSE]: MYSQL,
  [DatabaseTypeCode.COCKROACHDB]: MYSQL,
  [DatabaseTypeCode.DB2]: MYSQL,
  [DatabaseTypeCode.HIVE]: MYSQL,
  [DatabaseTypeCode.KYLIN]: MYSQL,
  [DatabaseTypeCode.OCEANBASE]: MYSQL,
  [DatabaseTypeCode.PRESTO]: MYSQL,
  [DatabaseTypeCode.SNOWFLAKE]: MYSQL,
  [DatabaseTypeCode.SQLITE]: MYSQL,
  [DatabaseTypeCode.SUNDB]: MYSQL,
  [DatabaseTypeCode.TIDB]: MYSQL,
  [DatabaseTypeCode.XUGUDB]: MYSQL,
  // oracle
  [DatabaseTypeCode.ORACLE]: ORACLE,
  [DatabaseTypeCode.DM]: ORACLE,
  [DatabaseTypeCode.OSCAR]: ORACLE,
  [DatabaseTypeCode.OCEANBASE_ORACLE]: ORACLE,
  // pgsql
  [DatabaseTypeCode.POSTGRESQL]: PG,
  [DatabaseTypeCode.KINGBASE]: PG,
  [DatabaseTypeCode.OPENGAUSS]: PG,
  // sqlserver
  [DatabaseTypeCode.SQLSERVER]: SQLServer,
};

export interface IKeyword {
  type: DatabaseTypeCode;
  priority_keywords: string[];
  keywords: string[];
  priority_functions: IFunction[];
  functions: IFunction[];
}

export interface IFunction {
  name: string;
  parameters: IFunctionParameter[];
  returnType: string;
  description: string;
  example?: string;
  insert_text?: string;
}

export interface IFunctionParameter {
  name: string;
  type: string;
  optional?: boolean;
  description?: string;
}
