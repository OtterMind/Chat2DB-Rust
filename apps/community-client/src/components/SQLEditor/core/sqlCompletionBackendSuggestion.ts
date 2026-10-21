import type * as monaco from 'monaco-editor';

export interface BackendCompletionLabelSource {
  label?: string;
  insertText?: string;
  type?: string;
  detail?: string;
  description?: string;
  dataType?: string;
  objectType?: string;
  tableAlias?: string;
  tableName?: string;
}

const COLUMN_LIKE_TYPES = new Set(['COLUMN', 'JOIN_CLAUSE']);
const QUALIFIED_REFERENCE_TYPES = new Set(['DATABASE', 'SCHEMA', 'TABLE', 'VIEW', 'ALIAS']);
const VARIABLE_TYPE = 'VARIABLE';

function wrapDetail(value?: string): string {
  return value ? `(${value})` : '';
}

function isColumnLikeCandidate(candidate: BackendCompletionLabelSource): boolean {
  return COLUMN_LIKE_TYPES.has(candidate.type || '');
}

export function getBackendCompletionItemLabel(
  candidate: BackendCompletionLabelSource,
): monaco.languages.CompletionItemLabel {
  const itemLabel = {
    label: candidate.label || candidate.insertText || '',
  } as monaco.languages.CompletionItemLabel;
  const detail = getBackendCompletionItemDetail(candidate);
  const description = getBackendCompletionItemDescription(candidate);
  if (detail) {
    itemLabel.detail = detail;
  }
  if (description) {
    itemLabel.description = description;
  }
  return itemLabel;
}

export function getBackendCompletionItemFilterText(candidate: BackendCompletionLabelSource): string {
  return candidate.label || candidate.insertText || '';
}

export function getBackendCompletionItemInsertText(
  candidate: BackendCompletionLabelSource,
  userVariableMarkerAlreadyTyped = false,
): string {
  const insertText = candidate.insertText || candidate.label || '';
  return candidate.type === VARIABLE_TYPE && userVariableMarkerAlreadyTyped && insertText.startsWith('@')
    ? insertText.substring(1)
    : insertText;
}

export function getBackendCompletionItemEffectiveFilterText(
  candidate: BackendCompletionLabelSource,
  insertText: string,
): string {
  const filterText = getBackendCompletionItemFilterText(candidate);
  return candidate.type === VARIABLE_TYPE && filterText.startsWith('@') && !insertText.startsWith('@')
    ? filterText.substring(1)
    : filterText;
}

export function shouldTriggerQualifiedReferenceCompletion(candidate: BackendCompletionLabelSource): boolean {
  return Boolean(
    candidate.insertText?.endsWith('.')
    && QUALIFIED_REFERENCE_TYPES.has(candidate.type || ''),
  );
}

export function getBackendCompletionItemDetail(candidate: BackendCompletionLabelSource): string {
  if (isColumnLikeCandidate(candidate)) {
    return wrapDetail(candidate.tableAlias || candidate.tableName);
  }
  if (candidate.type === VARIABLE_TYPE) {
    return '';
  }
  return candidate.detail || '';
}

export function getBackendCompletionItemDescription(candidate: BackendCompletionLabelSource): string {
  if (isColumnLikeCandidate(candidate)) {
    return candidate.dataType || candidate.description || candidate.detail || candidate.objectType || '';
  }
  if (candidate.type === VARIABLE_TYPE) {
    return candidate.dataType || '';
  }
  return candidate.description || '';
}
