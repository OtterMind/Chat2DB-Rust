import sqlSnippets from '../helper/snippets/common';
/**
 * registerSnippets.ts
 * ---------------------
 * Define SQL snippets and register them with Monaco Editor.
 */

import * as monaco from 'monaco-editor';
import { SORT_TEXT } from '../type';
import { useGlobalStore } from '@/store/global';
import { isBackendCompletionModel } from './sqlCompletionModelMode';

let isRegistered = false;

export function registerSQLSnippets() {
  if (isRegistered) {
    console.log('SQL snippets have already been registered.');
    return;
  }

  monaco.languages.registerCompletionItemProvider('sql', {
    provideCompletionItems: (
      model: monaco.editor.ITextModel,
      position: monaco.Position,
      // context: monaco.languages.CompletionContext,
      // token: monaco.CancellationToken,
    ): monaco.languages.ProviderResult<monaco.languages.CompletionList> => {
      if (isBackendCompletionModel(model)) {
        return undefined;
      }

      const word = model.getWordUntilPosition(position);
      const range: monaco.IRange = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };

      const keywordCase = useGlobalStore.getState().editorSettings.keywordCase;
      const suggestions: monaco.languages.CompletionItem[] = Object.keys(sqlSnippets).map((key) => {
        const snippet = sqlSnippets[key];
        return {
          label: {
            label: key,
            description: snippet.detail,
          },
          kind: monaco.languages.CompletionItemKind.Snippet,
          insertText: keywordCase ? snippet.snippet : snippet.snippet.toLowerCase(),
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          documentation: {
            value: `**${key}**\n\n${snippet.detail}`,
            isTrusted: true,
          },
          range,
          command: {
        // Trigger completion again after the current completion resolves.
            id: 'editor.action.triggerSuggest',
            arguments: [{ triggerCharacter: '.' }],
          },
          sortText: `${SORT_TEXT.SNIPPET}${key.padEnd(8, 'a')}`.toLowerCase(),
        } as monaco.languages.CompletionItem;
      });
      // console.log('suggestions', suggestions);
      return { suggestions };
    },
  });

  isRegistered = true;
  // console.log('SQL snippets have been registered successfully.');
}
