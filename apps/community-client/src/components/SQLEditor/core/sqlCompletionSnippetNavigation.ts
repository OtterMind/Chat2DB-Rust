export interface SnippetNavigationSnapshot {
  modelVersionId: number;
  cursor: number;
  selectionStart: number;
  selectionEnd: number;
}

export function shouldTriggerSnippetPlaceholderCompletion(
  before: SnippetNavigationSnapshot | null | undefined,
  after: SnippetNavigationSnapshot | null | undefined,
): boolean {
  if (!before || !after) {
    return false;
  }
  if (before.modelVersionId !== after.modelVersionId) {
    return false;
  }
  return before.cursor !== after.cursor
    || before.selectionStart !== after.selectionStart
    || before.selectionEnd !== after.selectionEnd;
}
