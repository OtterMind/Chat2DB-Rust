export enum NoSQLDataBaseType {
  MONGODB = 'MONGODB',
  REDIS = 'REDIS',
}
export enum DatabaseTypeCode {
  MYSQL = 'MYSQL',
  ORACLE = 'ORACLE',
  DB2 = 'DB2',
  MONGODB = 'MONGODB',
  REDIS = 'REDIS',
  H2 = 'H2',
  POSTGRESQL = 'POSTGRESQL',
  SQLSERVER = 'SQLSERVER',
  SQLITE = 'SQLITE',
  MARIADB = 'MARIADB',
  CLICKHOUSE = 'CLICKHOUSE',
  DM = 'DM',
  OSCAR = 'OSCAR',
  OCEANBASE = 'OCEANBASE',
  OCEANBASE_ORACLE = 'OCEANBASE_ORACLE',
  PRESTO = 'PRESTO',
  HIVE = 'HIVE',
  KINGBASE = 'KINGBASE',
  SNOWFLAKE = 'SNOWFLAKE',
  OPENGAUSS = 'OPENGAUSS',
  SUNDB = 'SUNDB',
  TIDB = 'TIDB',
  COCKROACHDB = 'COCKROACHDB',
  KYLIN = 'KYLIN',
  XUGUDB = 'XUGUDB',
  DUCKDB = 'DUCKDB',
  ELASTICSEARCH='ELASTICSEARCH',
  TDENGINE="TDENGINE",
  BIGQUERY="BIGQUERY",
  REDSHIFT="REDSHIFT",
  INFORMIX="INFORMIX",
  DORIS="DORIS",
  STARROCKS="STARROCKS",
  GAUSSDB="GAUSSDB",
  GBASE8S="GBASE8S",
}

export enum ConsoleStatus {
  DRAFT = 'DRAFT',
  RELEASE = 'RELEASE',
}

// Universal add, delete, modify, check enumeration
export enum CRUD {
  CREATE = 'CREATE',
  READ = 'READ',
  UPDATE = 'UPDATE',
  DELETE = 'DELETE',
  UPDATE_COPY = 'UPDATE_COPY',
}

// Service status
export enum ServiceStatus {
  PENDING = 'PENDING',
  SUCCESS = 'SUCCESS',
  FAILURE = 'FAILURE',
}

// Task status: not started, loading, finished
export enum TaskStatus {
  NOT_STARTED = 'NOT_STARTED',
  LOADING = 'LOADING',
  FINISH = 'FINISH',
}

// iframe type
export enum IframeType {
  ZOER = 'zoer',
  OPENGAUSS = 'opengauss',
}
