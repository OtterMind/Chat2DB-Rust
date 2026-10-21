/**
 * registerThemes.ts
 * ---------------------
 * Register themes.
 */
import * as monaco from 'monaco-editor';
import { editorThemes } from '../config';

/**
 * Register themes.
 */
export const registerThemes = () => {
  Object.entries(editorThemes).forEach(([name, theme]) => {
    if (typeof theme === 'object') {
      // console.log('register theme', name);
      monaco.editor.defineTheme(name, theme as monaco.editor.IStandaloneThemeData);
    }
  });
};
