export interface SqlFormatScope {
  datasourceKey: string;
  databaseType: string;
  sql: string;
}

export interface SqlFormatRequestToken {
  sequence: number;
  scope: SqlFormatScope;
}

export function isCurrentSqlFormatRequest(
  request: SqlFormatRequestToken,
  currentSequence: number,
  currentScope: SqlFormatScope,
): boolean {
  return request.sequence === currentSequence
    && request.scope.datasourceKey === currentScope.datasourceKey
    && request.scope.databaseType === currentScope.databaseType
    && request.scope.sql === currentScope.sql;
}
