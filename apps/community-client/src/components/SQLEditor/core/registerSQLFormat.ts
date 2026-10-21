/**
 * registerSQLFormat.ts
 * ---------------------
 * Register SQL formatting.
 */
import * as monaco from 'monaco-editor';

const registerSQLFormat = () => {
  monaco.languages.registerDocumentFormattingEditProvider('sql', {
    provideDocumentFormattingEdits: (model) => {
    // Implement formatting.
      return [
        {
          range: model.getFullModelRange(),
          text: '// Formatted code\n' + model.getValue(),
        },
      ];
    },
  });
};

export { registerSQLFormat };
