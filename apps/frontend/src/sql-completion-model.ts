export interface SqlCompletionScope {
  datasourceKey: string;
  databaseType: string;
  databaseName: string;
  schemaName: string;
  sql: string;
  cursorUtf16: number;
  refreshGeneration: number;
}

export interface SqlCompletionRequestToken {
  sequence: number;
  scope: SqlCompletionScope;
}

export interface SqlCompletionEditCandidate {
  label: string;
  insertText?: string | null;
  replaceStartUtf16?: number | null;
  replaceEndUtf16?: number | null;
}

export interface SqlCompletionReplacement {
  sql: string;
  caret: number;
}

export function isCurrentSqlCompletionRequest(
  request: SqlCompletionRequestToken,
  currentSequence: number,
  currentScope: SqlCompletionScope,
): boolean {
  return request.sequence === currentSequence
    && request.scope.datasourceKey === currentScope.datasourceKey
    && request.scope.databaseType === currentScope.databaseType
    && request.scope.databaseName === currentScope.databaseName
    && request.scope.schemaName === currentScope.schemaName
    && request.scope.sql === currentScope.sql
    && request.scope.cursorUtf16 === currentScope.cursorUtf16
    && request.scope.refreshGeneration === currentScope.refreshGeneration;
}

export function applySqlCompletion(
  sql: string,
  fallbackReplaceStartUtf16: number,
  fallbackReplaceEndUtf16: number,
  candidate: SqlCompletionEditCandidate,
): SqlCompletionReplacement | null {
  const replaceStart = candidate.replaceStartUtf16 ?? fallbackReplaceStartUtf16;
  const replaceEnd = candidate.replaceEndUtf16 ?? fallbackReplaceEndUtf16;
  const insertText = candidate.insertText ?? candidate.label;
  if (
    !Number.isSafeInteger(replaceStart)
    || !Number.isSafeInteger(replaceEnd)
    || replaceStart < 0
    || replaceEnd < replaceStart
    || replaceEnd > sql.length
  ) return null;

  return {
    sql: `${sql.slice(0, replaceStart)}${insertText}${sql.slice(replaceEnd)}`,
    caret: replaceStart + insertText.length,
  };
}

export function moveSqlCompletionSelection(
  currentIndex: number,
  candidateCount: number,
  direction: -1 | 1,
): number {
  if (candidateCount <= 0) return -1;
  const normalized = currentIndex >= 0 && currentIndex < candidateCount ? currentIndex : 0;
  return (normalized + direction + candidateCount) % candidateCount;
}
