import * as monaco from 'monaco-editor';

let completionList: monaco.IDisposable | null = null;

/**
 * Initialize completions.
 */
export function initCompletionProvider() {
  completionList = monaco.languages.registerCompletionItemProvider('sql', {
    provideCompletionItems: () => ({
      suggestions: [],
    }),
  });
}

/**
 * Reset completions.
 */
export function resetCompletionProviders() {
  if (completionList) {
    completionList.dispose();
    completionList = null;
  }
}

export function registerCompletionProvider() {
  completionList = monaco.languages.registerCompletionItemProvider('sql', {
    provideCompletionItems: () => {
      return { suggestions: [] };
    },
  });
}
