import { useEffect } from 'react';
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import { createStyles } from 'antd-style';
import { dark } from './editorTheme/dark';
import { light } from './editorTheme/light';
import { rules } from './editorTheme/rules';

const useStyles = createStyles(() => {
  return {};
});

// Normalize colors to six-digit hex values such as #111111.
// For example, convert #111 to #111111 and rgba(...) to #xxxxxx.
function transformColor(color: string) {
  // Handle rgba format.
  if (color.startsWith('rgba')) {
    const rgba = color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*[\d.]+)?\)/);
    if (rgba) {
      const r = parseInt(rgba[1]).toString(16)
.padStart(2, '0');
      const g = parseInt(rgba[2]).toString(16)
.padStart(2, '0');
      const b = parseInt(rgba[3]).toString(16)
.padStart(2, '0');
      return `#${r}${g}${b}`;
    }
  }

  // Handle short hexadecimal colors.
  if (color.length === 4 && color.startsWith('#')) {
    return color
      .split('')
      .map((item, index) => (index !== 0 ? item + item : item))
      .join('');
  }
  return color;
}

// Clear the selection when the user clicks a non-copyable element.
function useMonacoTheme() {
  const { theme } = useStyles();
  const appearance = theme.appearance;
  const { colorPrimary, colorPrimaryBg, colorBgBase, colorText, colorTextQuaternary, colorFill } =
    theme;

  useEffect(() => {
    const colors = {
      ...(appearance.includes('dark') ? dark : light),
      'editor.lineHighlightBackground': transformColor(colorPrimaryBg), // Current-line background.
      'editor.selectionBackground': transformColor(colorPrimary) + '50', // Selection background.
      'editorLineNumber.foreground': colorTextQuaternary, // Line-number color.
      'editorLineNumber.activeForeground': colorPrimary, // Active line-number color.
      'editorCursor.foreground': colorPrimary, // Cursor color.
      'editorRuler.foreground': transformColor(colorPrimary) + '15', // Ruler color.
      'editor.foreground': transformColor(colorText), // Text color.
      'editor.background': transformColor(colorBgBase), // Background color.
      'editorSuggestWidget.background': transformColor(colorBgBase), // Completion background.
      'editorSuggestWidget.highlightForeground': transformColor(colorText), // Completion highlight color.
      'editorSuggestWidget.selectedBackground': transformColor(colorFill) + '50', // Selected completion background.
      'list.hoverBackground': transformColor(colorFill) + '50', // Completion hover background.
      'list.highlightForeground': transformColor(colorPrimary), // Completion highlight foreground.
      // 'scrollbarSlider.background': colorFill,
      // 'scrollbarSlider.hoverBackground': colorFill,
      // 'scrollbarSlider.activeBackground': colorFill,
    };


    const themeName = appearance.includes('dark') ? 'dark' : 'light';

    monaco.editor.defineTheme(themeName, {
      // Use vs-dark for dark appearance; otherwise use vs.
      base: appearance.includes('dark') ? 'vs-dark' : 'vs',
      inherit: true, // Inherit the default VS Code theme.
      rules: [
        ...rules,
        { token: 'string.sql', foreground: '#98c379' },
        { background: '#15161a' },
        // { token: 'comment', foreground: '#7f848e' },
      ] as any,
      colors,
    });

    // monaco.languages.setMonarchTokensProvider('sql', {
    //   tokenizer: {
    //     root: [
    //       [/\b(parameter_key)\b/, 'keyword'],
    //     ]
    //   }
    // });

    monaco.editor.setTheme(themeName);
  }, [appearance]);
}

export default useMonacoTheme;
