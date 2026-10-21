import { ISqlEditorHintRangeVO, ISqlEditorHintVO } from '@/typings/sqlParser';

export interface SqlCompletionHintScope {
  key: string;
  range?: ISqlEditorHintRangeVO;
}

interface HintBucket {
  scope: SqlCompletionHintScope;
  hints: ISqlEditorHintVO[];
}

export class SqlCompletionHintStore {
  private readonly buckets = new Map<string, HintBucket>();

  public getHints(): ISqlEditorHintVO[] {
    return Array.from(this.buckets.values()).flatMap((bucket) => bucket.hints);
  }

  public clear(): ISqlEditorHintVO[] {
    this.buckets.clear();
    return [];
  }

  public commitHints(editorHints: ISqlEditorHintVO[] | null | undefined): ISqlEditorHintVO[] {
    this.buckets.clear();
    this.upsertHints(editorHints);
    return this.getHints();
  }

  public commitScoped(
    scope: SqlCompletionHintScope | null | undefined,
    editorHints: ISqlEditorHintVO[] | null | undefined,
  ): ISqlEditorHintVO[] {
    if (!scope) {
      return this.commitHints(editorHints);
    }
    this.deleteScope(scope);
    this.upsertHints(editorHints);
    return this.getHints();
  }

  private upsertHints(editorHints: ISqlEditorHintVO[] | null | undefined): void {
    (editorHints || []).forEach((hint, index) => {
      const scope = scopeFromEditorHint(hint, index);
      this.buckets.set(scope.key, {
        scope,
        hints: [hint],
      });
    });
  }

  private deleteScope(scope: SqlCompletionHintScope): void {
    Array.from(this.buckets.entries()).forEach(([key, bucket]) => {
      if (isSameScope(bucket.scope, scope) || isIntersectingScope(bucket.scope, scope)) {
        this.buckets.delete(key);
      }
    });
  }
}

export function createSqlCompletionHintStore(): SqlCompletionHintStore {
  return new SqlCompletionHintStore();
}

export function sqlCompletionHintScopeFromRange(
  prefix: string,
  range: ISqlEditorHintRangeVO | null | undefined,
): SqlCompletionHintScope | null {
  if (!isValidRange(range)) {
    return null;
  }
  return {
    key: `${prefix}:${rangeKey(range)}`,
    range,
  };
}

function scopeFromEditorHint(editorHint: ISqlEditorHintVO, index: number): SqlCompletionHintScope {
  const range =
    editorHint.rowRange || editorHint.valueRange || itemRangesEnvelope(editorHint) || editorHint.statementRange;
  return sqlCompletionHintScopeFromRange(editorHint.type || 'UNKNOWN', range) || {
    key: `${editorHint.type || 'UNKNOWN'}:unscoped:${index}`,
  };
}

function isSameScope(scopeA: SqlCompletionHintScope, scopeB: SqlCompletionHintScope): boolean {
  return scopeA.key === scopeB.key;
}

function isIntersectingScope(scopeA: SqlCompletionHintScope, scopeB: SqlCompletionHintScope): boolean {
  if (!scopeA.range || !scopeB.range) {
    return false;
  }
  return isRangeIntersecting(scopeA.range, scopeB.range);
}

function isRangeIntersecting(rangeA: ISqlEditorHintRangeVO, rangeB: ISqlEditorHintRangeVO): boolean {
  if (isPointRange(rangeA)) {
    return isPointInsideRange(rangeA, rangeB);
  }
  if (isPointRange(rangeB)) {
    return isPointInsideRange(rangeB, rangeA);
  }
  return (
    comparePosition(rangeA.startLineNumber, rangeA.startColumn, rangeB.endLineNumber, rangeB.endColumn) < 0 &&
    comparePosition(rangeB.startLineNumber, rangeB.startColumn, rangeA.endLineNumber, rangeA.endColumn) < 0
  );
}

function isPointRange(range: ISqlEditorHintRangeVO): boolean {
  return comparePosition(range.startLineNumber, range.startColumn, range.endLineNumber, range.endColumn) === 0;
}

function isPointInsideRange(pointRange: ISqlEditorHintRangeVO, range: ISqlEditorHintRangeVO): boolean {
  if (isPointRange(range)) {
    return isSamePosition(pointRange, range);
  }
  return (
    comparePosition(range.startLineNumber, range.startColumn, pointRange.startLineNumber, pointRange.startColumn) <=
      0 &&
    comparePosition(pointRange.startLineNumber, pointRange.startColumn, range.endLineNumber, range.endColumn) < 0
  );
}

function isSamePosition(rangeA: ISqlEditorHintRangeVO, rangeB: ISqlEditorHintRangeVO): boolean {
  return rangeA.startLineNumber === rangeB.startLineNumber && rangeA.startColumn === rangeB.startColumn;
}

function isValidRange(range: ISqlEditorHintRangeVO | null | undefined): range is ISqlEditorHintRangeVO {
  return (
    !!range &&
    range.startLineNumber > 0 &&
    range.startColumn > 0 &&
    range.endLineNumber > 0 &&
    range.endColumn > 0 &&
    comparePosition(range.startLineNumber, range.startColumn, range.endLineNumber, range.endColumn) <= 0
  );
}

function rangeKey(range: ISqlEditorHintRangeVO): string {
  return `${range.startLineNumber}:${range.startColumn}-${range.endLineNumber}:${range.endColumn}`;
}

function itemRangesEnvelope(editorHint: ISqlEditorHintVO): ISqlEditorHintRangeVO | null {
  const ranges = (editorHint.items || [])
    .map((item) => item.range)
    .filter(isValidRange);
  if (!ranges.length) {
    return null;
  }
  return ranges.reduce((acc, range) => ({
    startLineNumber:
      comparePosition(range.startLineNumber, range.startColumn, acc.startLineNumber, acc.startColumn) < 0
        ? range.startLineNumber
        : acc.startLineNumber,
    startColumn:
      comparePosition(range.startLineNumber, range.startColumn, acc.startLineNumber, acc.startColumn) < 0
        ? range.startColumn
        : acc.startColumn,
    endLineNumber:
      comparePosition(range.endLineNumber, range.endColumn, acc.endLineNumber, acc.endColumn) > 0
        ? range.endLineNumber
        : acc.endLineNumber,
    endColumn:
      comparePosition(range.endLineNumber, range.endColumn, acc.endLineNumber, acc.endColumn) > 0
        ? range.endColumn
        : acc.endColumn,
  }));
}

function comparePosition(lineA: number, colA: number, lineB: number, colB: number): number {
  if (lineA !== lineB) {
    return lineA - lineB;
  }
  return colA - colB;
}
