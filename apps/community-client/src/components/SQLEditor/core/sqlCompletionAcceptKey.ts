import type * as monaco from 'monaco-editor';
import type { SqlCompletionAcceptKey } from '../type';

export const DEFAULT_SQL_COMPLETION_ACCEPT_KEY: SqlCompletionAcceptKey = 'enter';
export const SQL_COMPLETION_ACCEPT_KEY_ENTER_CONTEXT = 'chat2dbSqlCompletionAcceptKeyEnter';
export const SQL_COMPLETION_ACCEPT_KEY_TAB_CONTEXT = 'chat2dbSqlCompletionAcceptKeyTab';

export const getSqlCompletionAcceptKey = (acceptKey?: SqlCompletionAcceptKey | null): SqlCompletionAcceptKey => {
  return acceptKey === 'tab' ? 'tab' : DEFAULT_SQL_COMPLETION_ACCEPT_KEY;
};

export const getSqlCompletionAcceptKeyOptions = (
  acceptKey?: SqlCompletionAcceptKey | null,
): Pick<monaco.editor.IStandaloneEditorConstructionOptions, 'acceptSuggestionOnEnter' | 'tabCompletion'> => {
  const effectiveAcceptKey = getSqlCompletionAcceptKey(acceptKey);
  return {
    acceptSuggestionOnEnter: effectiveAcceptKey === 'enter' ? 'on' : 'off',
    tabCompletion: effectiveAcceptKey === 'tab' ? 'on' : 'off',
  };
};
